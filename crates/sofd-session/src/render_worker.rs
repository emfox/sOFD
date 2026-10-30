//! Rasterization off the draw path.
//! Rendering can take long enough to drop a frame, and the reader's budget is
//! that the interface never blocks, so the worker rasterizes on its own thread.

use std::sync::mpsc::{Receiver, Sender};

use crate::viewer::{self, RenderedPage};

/// What has been asked for on a page: the resolution, and the visible area in
/// whole millimetres, which is what makes two requests the same request.
type Asked = (f32, u32, Option<(i64, i64, i64, i64)>);

/// One unit of work: rasterize `page` at `dpi`, as generation `generation`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderRequest {
    /// Monotonic stamp; a newer stamp supersedes every older one.
    pub generation: u64,
    /// Page index to rasterize.
    pub page: usize,
    /// Resolution to rasterize at, before the texture limit is applied.
    pub dpi: f32,
    /// The part of the page on screen, in millimetres; `None` asks for it all.
    pub window: Option<sofd_common::Rect>,
    /// The largest texture the backend can hold, in pixels.
    pub limit: u32,
}

/// A finished raster, carrying the generation it was requested under.
pub struct RenderResult {
    /// The generation of the request that produced this.
    pub generation: u64,
    /// Page index, so the caller can key its cache.
    pub page: usize,
    /// Resolution it was rendered at.
    pub dpi: f32,
    /// The decoded page, or the error that stopped it.
    pub outcome: Result<RenderedPage, String>,
}

/// The reader's half of the channel pair.
pub struct RenderQueue {
    to_worker: Sender<RenderRequest>,
    from_worker: Receiver<RenderResult>,
    /// The newest generation handed out.
    newest: u64,
    /// What has been asked for and not yet answered, so an identical repeat
    /// request can be recognised instead of restamped. The window is part of
    /// the key: scrolling a zoomed page asks for different millimetres.
    in_flight: std::collections::HashMap<usize, Asked>,
    /// The newest generation the worker has answered, if any.
    answered: Option<u64>,
}

impl RenderQueue {
    /// Start a worker over a copy of `bytes`, where the document came from.
    ///
    /// The thread ends when the queue is dropped and the channel closes.
    pub fn spawn(bytes: Vec<u8>) -> Self {
        let (to_worker, work) = std::sync::mpsc::channel::<RenderRequest>();
        let (finish, from_worker) = std::sync::mpsc::channel::<RenderResult>();
        // The worker keeps its own sender so it can put back the requests it
        // looked at but did not consume; without this a peek would either eat
        // them or need a second channel.
        let own_to_worker = to_worker.clone();
        std::thread::Builder::new()
            .name("sofd-render".to_string())
            .spawn(move || run_worker(bytes, &work, &own_to_worker, &finish))
            .expect("the render worker thread starts");
        Self {
            to_worker,
            from_worker,
            newest: 0,
            in_flight: std::collections::HashMap::new(),
            answered: None,
        }
    }

    /// The generation currently being asked for.
    pub fn generation(&self) -> u64 {
        self.newest
    }

    /// Whether the newest request is still outstanding.
    pub fn is_busy(&self) -> bool {
        self.answered != Some(self.newest)
    }

    /// Forget what is in flight, because the resolution changed.
    /// A raster of the old resolution that lands now is still kept, under its
    /// own resolution; it is simply not the one on screen.
    pub fn rezoom(&mut self) {
        self.in_flight.clear();
        self.newest += 1;
        self.answered = Some(self.newest);
    }

    /// Ask for a page. Returns the generation the request was stamped with.
    /// Asking twice for the same page at the same resolution is not new
    /// information, and honouring every repeat would starve the worker.
    pub fn request(
        &mut self,
        page: usize,
        dpi: f32,
        window: Option<sofd_common::Rect>,
        limit: u32,
    ) -> u64 {
        // Rounded to whole millimetres, so a scroll of a tenth of one does not
        // restamp a request the worker is already answering.
        let key = window.map(|w| {
            (
                w.x.round() as i64,
                w.y.round() as i64,
                w.w.round() as i64,
                w.h.round() as i64,
            )
        });
        if self
            .in_flight
            .get(&page)
            .is_some_and(|asked| *asked == (dpi, limit, key))
        {
            return self.newest;
        }
        self.newest += 1;
        self.in_flight.insert(page, (dpi, limit, key));
        let request = RenderRequest {
            generation: self.newest,
            page,
            dpi,
            window,
            limit,
        };
        // A closed worker means the thread is gone; the caller then simply
        // never sees a result, which reads as "still pending" and is safe.
        let _ = self.to_worker.send(request);
        self.newest
    }

    /// Everything the worker has finished since the last call. A stale raster
    /// is handed back too: it is cached under its own resolution and window,
    /// so discarding it would leave the page blank at that zoom.
    pub fn drain(&mut self) -> Vec<RenderResult> {
        let mut done = Vec::new();
        while let Ok(result) = self.from_worker.try_recv() {
            self.answered = Some(match self.answered {
                Some(seen) => seen.max(result.generation),
                None => result.generation,
            });
            self.in_flight.remove(&result.page);
            done.push(result);
        }
        done
    }
}

/// The worker loop: recv a request and render it, skipping only work a newer
/// request for the same page has already made pointless. A request is never
/// skipped when it is the last word on its page, or nothing would answer it.
fn run_worker(
    bytes: Vec<u8>,
    work: &Receiver<RenderRequest>,
    own_to_worker: &Sender<RenderRequest>,
    finish: &Sender<RenderResult>,
) {
    let mut document = None;
    let mut seals: Option<Vec<sofd::Seal>> = None;
    let mut pending: Option<RenderRequest> = None;
    loop {
        // Hold the request we are about to render, and look at whatever has
        // already been queued behind it. Peeking does not consume: `pending`
        // carries forward, so nothing is ever dropped on the floor.
        let request = match pending.take() {
            Some(request) => request,
            None => match work.recv() {
                Ok(request) => request,
                Err(_) => return,
            },
        };
        let queued = drain_queue(work);
        let superseded = queued
            .iter()
            .any(|later| later.page == request.page && later.generation > request.generation);
        if superseded {
            // The newest queued request for this page becomes the next one
            // rendered; the rest of the queue is put back untouched.
            let newest_for_page = queued
                .iter()
                .filter(|later| later.page == request.page)
                .max_by_key(|later| later.generation)
                .copied()
                .expect("a newer request for this page was just found");
            let rest = queued
                .iter()
                .copied()
                .filter(|later| *later != newest_for_page);
            pending = Some(newest_for_page);
            for later in rest.rev() {
                let _ = own_to_worker.send(later);
            }
            continue;
        }
        // Everything queued behind this request has to be rendered too, so put
        // it back in the order it arrived: a channel is a FIFO, so pushing in
        // reverse restores the original sequence.
        for later in queued.iter().rev() {
            let _ = own_to_worker.send(*later);
        }
        if document.is_none() {
            match sofd::Document::open_bytes(&bytes) {
                Ok(doc) => document = Some(doc),
                Err(err) => {
                    let _ = finish.send(RenderResult {
                        generation: request.generation,
                        page: request.page,
                        dpi: request.dpi,
                        outcome: Err(err.to_string()),
                    });
                    continue;
                }
            }
        }
        // The seals are read once per document, not per page: they travel with
        // the raster so a page shows the stamps that cover it.
        if seals.is_none() {
            seals = Some(
                document
                    .as_mut()
                    .and_then(|d| d.seals().ok())
                    .unwrap_or_default(),
            );
        }
        let doc = document.as_ref().expect("opened just above");
        let stamps = seals.as_deref().unwrap_or(&[]);
        let outcome = viewer::render_page_with_seals(
            doc,
            request.page,
            request.dpi,
            stamps,
            request.window,
            request.limit,
        )
        .map_err(|e| e.to_string());
        let _ = finish.send(RenderResult {
            generation: request.generation,
            page: request.page,
            dpi: request.dpi,
            outcome,
        });
    }
}

/// Take everything currently queued, stopping at the first empty read.
fn drain_queue(work: &Receiver<RenderRequest>) -> Vec<RenderRequest> {
    let mut queued = Vec::new();
    while let Ok(request) = work.try_recv() {
        queued.push(request);
    }
    queued
}
