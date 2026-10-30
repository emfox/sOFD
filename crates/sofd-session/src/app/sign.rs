//! Signatures: verification and the panel that reports it.

use super::*;

impl App {
    /// Whether the signature panel is showing.
    pub fn signatures_open(&self) -> bool {
        self.signatures_open
    }

    /// Show or hide the signature panel, verifying when it first opens.
    /// Verification reads and hashes the whole package, so it is done once and
    /// kept rather than on every frame; closing it keeps the answer.
    pub fn set_signatures_open(&mut self, open: bool) {
        self.signatures_open = open;
        if open && self.signatures.is_none() {
            self.verify_signatures();
        }
    }

    /// Verify every signature in the document and remember the results.
    /// A document with no signatures is unsigned, not broken, so the absence
    /// is an empty list; one that cannot be checked is a third verdict.
    pub fn verify_signatures(&mut self) {
        let Some(doc) = self.doc.as_mut() else {
            self.signatures = Some(Vec::new());
            return;
        };
        let declared = match doc.signatures() {
            Ok(refs) => refs,
            Err(_) => {
                self.signatures = Some(Vec::new());
                return;
            }
        };
        // The verdicts come back in declaration order, so a failure is matched
        // to the signature it belongs to; and one pass yields them all, where
        // asking per signature would re-read the package each time.
        let verdicts = doc.verify_signatures();
        // The details cost a second read of the container, which is why they
        // are taken here, once, rather than each time a dialog opens.
        let details = doc.signature_details().unwrap_or_default();
        // The seals are what a reader right-clicks to check an invoice
        // (notice 2020 no. 1). They are read as drawn seals, so the box a
        // click has to land in is the box the seal was pasted into.
        let seals = doc.seals().unwrap_or_default();
        self.signatures = Some(
            declared
                .iter()
                .enumerate()
                .map(|(index, sig)| {
                    let verdict = verdicts
                        .as_ref()
                        .ok()
                        .and_then(|statuses| statuses.get(index).copied())
                        .map_or(Verdict::Unverified, Verdict::from_status);
                    let detail = details.get(index).cloned().unwrap_or_default();
                    // The drawn seals are already joined to page indices, and
                    // `Seal.id` is a `StampAnnot` id, so it pairs the two.
                    let own: Vec<SealBox> = if detail.stamps.is_empty() {
                        Vec::new()
                    } else {
                        seals
                            .iter()
                            .filter(|seal| {
                                detail.stamps.iter().any(|stamp| {
                                    stamp.id == seal.id && stamp.boundary == seal.boundary
                                })
                            })
                            .map(|seal| SealBox {
                                page: seal.page,
                                boundary: seal.boundary,
                            })
                            .collect()
                    };
                    SignatureReport {
                        id: sig.id.clone(),
                        verdict,
                        signer: detail.common_name.clone(),
                        signed_at: detail.signed_at.clone(),
                        algorithm: detail.check_method.clone(),
                        details: detail,
                        seals: own,
                    }
                })
                .collect(),
        );
    }

    /// The verification results, or an empty list if none has been asked for.
    pub fn signatures(&self) -> &[SignatureReport] {
        self.signatures.as_deref().unwrap_or(&[])
    }

    /// How many signatures the open document carries, whether or not verified.
    pub fn signature_count(&self) -> usize {
        self.signatures().len()
    }

    /// The verdict text for one signature.
    pub fn signature_status_label(&self, report: &SignatureReport) -> String {
        let key = match report.verdict {
            Verdict::Valid => MessageId::SignValid,
            Verdict::Tampered => MessageId::SignTampered,
            Verdict::Unverified => MessageId::SignUnverified,
            Verdict::Untrusted => MessageId::SignUntrusted,
        };
        self.i18n.text(key)
    }

    /// The heading on the signature panel.
    pub fn signatures_heading(&self) -> String {
        self.i18n.text(MessageId::SignPanelTitle)
    }

    /// How many signatures the document has, in the reader's language.
    pub fn signature_count_label(&self) -> String {
        self.i18n.text_with(
            MessageId::SignCount,
            [("count", self.signature_count().to_string().into())],
        )
    }

    /// The signer line for one signature, when the document names one.
    pub fn signature_signer_label(&self, report: &SignatureReport) -> Option<String> {
        report.signer.as_ref().map(|name| {
            self.i18n
                .text_with(MessageId::SignSigner, [("name", name.clone().into())])
        })
    }

    /// The signing-time line for one signature, when the document records one.
    pub fn signature_time_label(&self, report: &SignatureReport) -> Option<String> {
        report.signed_at.as_ref().map(|time| {
            self.i18n
                .text_with(MessageId::SignSignedAt, [("time", time.clone().into())])
        })
    }

    /// The algorithm line for one signature, when the descriptor names one.
    pub fn signature_algorithm_label(&self, report: &SignatureReport) -> Option<String> {
        report.algorithm.as_ref().map(|algo| {
            self.i18n
                .text_with(MessageId::SignAlgorithm, [("algo", algo.clone().into())])
        })
    }

    /// The issuer line for one signature, when the certificate names one.
    pub fn signature_issuer_label(&self, report: &SignatureReport) -> Option<String> {
        report.details.issuer.as_ref().map(|issuer| {
            self.i18n
                .text_with(MessageId::SignIssuer, [("issuer", issuer.clone().into())])
        })
    }

    /// The certificate's validity window, when it states one.
    pub fn signature_validity_label(&self, report: &SignatureReport) -> Option<String> {
        let from = report.details.not_before.as_ref()?;
        let to = report.details.not_after.as_ref()?;
        Some(self.i18n.text_with(
            MessageId::SignValidUntil,
            [("from", from.clone().into()), ("to", to.clone().into())],
        ))
    }

    /// How much of the container the signature protects. The count of files
    /// outside every signature is reported even when it is zero, because a
    /// zero is the answer a reader is looking for.
    pub fn signature_coverage_label(&self, report: &SignatureReport) -> String {
        self.i18n.text_with(
            MessageId::SignCovers,
            [
                ("count", (report.details.covered as i64).into()),
                ("loose", (report.details.uncovered.len() as i64).into()),
            ],
        )
    }

    /// The three lines an invoice seal prints, when the certificate is one.
    /// This is what a reader checks against the paper in front of them; the
    /// text travels inside the certificate, so it cannot have been retyped.
    pub fn signature_seal_label(&self, report: &SignatureReport) -> Option<String> {
        let seal = report.details.seal.as_ref()?;
        Some(
            self.i18n
                .text_with(MessageId::SignSealText, [("text", seal.row().into())]),
        )
    }

    /// Which signature draws the seal under this point of this page, if any.
    ///
    /// The point is in page space, in millimetres, so the answer does not
    /// depend on the zoom: a seal is where the document put it. The box is the
    /// whole seal rather than its ink, because a click on the empty middle of a
    /// ring is still a click on the seal.
    pub fn seal_at(&self, page: usize, point: (f64, f64)) -> Option<usize> {
        self.signatures().iter().position(|report| {
            report.seals.iter().any(|seal| {
                seal.page == page
                    && point.0 >= seal.boundary.x
                    && point.1 >= seal.boundary.y
                    && point.0 <= seal.boundary.x + seal.boundary.w
                    && point.1 <= seal.boundary.y + seal.boundary.h
            })
        })
    }

    /// The signature a seal belongs to, found by page and position. Used by the
    /// hit test, which knows the box but not which signature drew it.
    pub fn seal_box(&self, index: usize) -> &[SealBox] {
        self.signatures()
            .get(index)
            .map(|report| report.seals.as_slice())
            .unwrap_or_default()
    }
}
