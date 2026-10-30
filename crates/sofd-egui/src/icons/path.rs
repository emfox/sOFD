//! Reading the `d` of an SVG path into subpaths of lines and cubics. The icon
//! set is stored as the path data the theme ships, so this is the only reader
//! there is; it understands exactly the commands those paths use.

/// One verb of a subpath, in the 16-unit space the icon theme draws on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// A straight line to a point.
    Line((f32, f32)),
    /// A cubic Bezier: two control points and the point it ends on.
    Cubic((f32, f32), (f32, f32), (f32, f32)),
}

/// A run of segments that shares a start, and whether the path closes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Subpath {
    /// Where the run begins.
    pub start: (f32, f32),
    /// The verbs, in the order the path gives them.
    pub segments: Vec<Segment>,
    /// Whether the path returns to `start` before ending the run.
    pub closed: bool,
}

/// Read path data into subpaths. Understands `M`, `L`, `H`, `V`, `C` and `Z`,
/// absolute or relative, which is what the icon theme's paths use; anything else
/// is reported rather than guessed at, so a path that grows one is seen.
pub fn parse(d: &str) -> Result<Vec<Subpath>, ParseError> {
    let mut reader = Reader::new(d);
    let mut out: Vec<Subpath> = Vec::new();
    let mut at = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    while let Some(command) = reader.command() {
        let relative = command.is_ascii_lowercase();
        let place = |from: (f32, f32), (x, y): (f32, f32)| {
            if relative {
                (from.0 + x, from.1 + y)
            } else {
                (x, y)
            }
        };
        match command.to_ascii_uppercase() {
            'M' => {
                let point = place(at, reader.pair()?);
                at = point;
                start = point;
                out.push(Subpath {
                    start,
                    segments: Vec::new(),
                    closed: false,
                });
                // A further pair after a moveto is a lineto, as the grammar says.
                while reader.peek_number() {
                    let point = place(at, reader.pair()?);
                    push(&mut out, Segment::Line(point));
                    at = point;
                }
            }
            'L' => loop {
                let point = place(at, reader.pair()?);
                push(&mut out, Segment::Line(point));
                at = point;
                if !reader.peek_number() {
                    break;
                }
            },
            'H' => loop {
                let x = reader.number()?;
                let point = (if relative { at.0 + x } else { x }, at.1);
                push(&mut out, Segment::Line(point));
                at = point;
                if !reader.peek_number() {
                    break;
                }
            },
            'V' => loop {
                let y = reader.number()?;
                let point = (at.0, if relative { at.1 + y } else { y });
                push(&mut out, Segment::Line(point));
                at = point;
                if !reader.peek_number() {
                    break;
                }
            },
            'C' => loop {
                let c1 = place(at, reader.pair()?);
                let c2 = place(at, reader.pair()?);
                let end = place(at, reader.pair()?);
                push(&mut out, Segment::Cubic(c1, c2, end));
                at = end;
                if !reader.peek_number() {
                    break;
                }
            },
            'Z' => {
                if let Some(current) = out.last_mut() {
                    current.closed = true;
                }
                at = start;
            }
            other => return Err(ParseError::Unsupported(other)),
        }
    }
    Ok(out)
}

/// Add a segment to the subpath being built.
fn push(out: &mut [Subpath], segment: Segment) {
    if let Some(current) = out.last_mut()
        && !current.closed
    {
        current.segments.push(segment);
    }
}

/// Why a path could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A number was expected and the data ran out.
    Truncated,
    /// A command this build does not carry.
    Unsupported(char),
}

/// One path's data, split into commands and numbers.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a str) -> Self {
        Self {
            bytes: d.as_bytes(),
            at: 0,
        }
    }

    /// The next command letter, skipping separators.
    fn command(&mut self) -> Option<char> {
        self.skip_separators();
        let byte = *self.bytes.get(self.at)?;
        if byte.is_ascii_alphabetic() {
            self.at += 1;
            Some(byte as char)
        } else {
            None
        }
    }

    /// Whether a number follows, which means the current command repeats.
    fn peek_number(&mut self) -> bool {
        self.skip_separators();
        matches!(self.bytes.get(self.at), Some(b) if *b == b'-' || *b == b'+' || *b == b'.' || b.is_ascii_digit())
    }

    /// One coordinate pair.
    fn pair(&mut self) -> Result<(f32, f32), ParseError> {
        Ok((self.number()?, self.number()?))
    }

    /// One number, in the plain decimal form the theme's paths are written in.
    fn number(&mut self) -> Result<f32, ParseError> {
        self.skip_separators();
        let start = self.at;
        if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
            self.at += 1;
        }
        if matches!(self.bytes.get(self.at), Some(b'.')) {
            self.at += 1;
            while matches!(self.bytes.get(self.at), Some(b) if b.is_ascii_digit()) {
                self.at += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
        text.parse().map_err(|_| ParseError::Truncated)
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.bytes.get(self.at),
            Some(b' ' | b',' | b'\t' | b'\n' | b'\r')
        ) {
            self.at += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_one_closed_subpath() {
        let paths = parse("m 1 2 h 4 v 4 h -4 z").expect("reads");
        assert_eq!(paths.len(), 1);
        let path = &paths[0];
        assert_eq!(path.start, (1.0, 2.0));
        assert!(path.closed);
        assert_eq!(
            path.segments,
            vec![
                Segment::Line((5.0, 2.0)),
                Segment::Line((5.0, 6.0)),
                Segment::Line((1.0, 6.0)),
            ]
        );
    }

    #[test]
    fn a_curve_keeps_both_control_points() {
        let paths = parse("M 0 0 C 1 0 2 1 3 1").expect("reads");
        assert_eq!(
            paths[0].segments,
            vec![Segment::Cubic((1.0, 0.0), (2.0, 1.0), (3.0, 1.0))]
        );
    }

    #[test]
    fn numbers_may_omit_the_separator() {
        let paths = parse("m 3-2 1.5.5").expect("reads");
        // `3-2` is two numbers, and `.5` follows `1.5` with no comma.
        assert_eq!(paths[0].start, (3.0, -2.0));
        assert_eq!(paths[0].segments, vec![Segment::Line((4.5, -1.5))]);
    }

    #[test]
    fn an_unknown_command_is_reported() {
        assert_eq!(parse("M 0 0 S 1 1 2 2"), Err(ParseError::Unsupported('S')));
    }
}
