//! Bounded paint commands for already-laid-out geometry.
//!
//! No CSS layout, font shaping or glyph generation happens here. Producers
//! supply device-pixel rectangles and glyph coverage masks. Commands retain
//! source/action IDs for hit testing elsewhere; rasterization only paints.

pub const MAX_COMMANDS: usize = 4_096;
pub const MAX_CLIP_DEPTH: usize = 32;
pub const MAX_GLYPH_BYTES: usize = 1_048_576;
pub const MAX_PIXELS: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Source {
    /// Index in the producer's source arena, if any.
    pub node: Option<usize>,
    /// Index in the producer's action table, if any.
    pub action: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Fill {
        rect: Rect,
        color: Rgb,
        source: Source,
    },
    /// A caller-supplied, row-major 8-bit coverage mask for a shaped glyph
    /// run. The raster does not pretend that this mask is CSS text layout.
    GlyphRun {
        bounds: Rect,
        coverage: Vec<u8>,
        color: Rgb,
        source: Source,
    },
    PushClip(Rect),
    PopClip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    TooManyCommands,
    TooManyClips,
    UnbalancedClip,
    GlyphBudget,
    InvalidCoverage,
    InvalidSurface,
    Allocation,
}

#[derive(Default)]
pub struct DisplayList {
    commands: Vec<Command>,
    glyph_bytes: usize,
    clip_depth: usize,
}

impl DisplayList {
    #[must_use]
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    #[must_use]
    pub fn glyph_bytes(&self) -> usize {
        self.glyph_bytes
    }

    fn append(&mut self, command: Command) -> Result<(), Error> {
        if self.commands.len() >= MAX_COMMANDS {
            return Err(Error::TooManyCommands);
        }
        self.commands
            .try_reserve_exact(1)
            .map_err(|_| Error::Allocation)?;
        self.commands.push(command);
        Ok(())
    }

    /// # Errors
    /// Fails at the command limit or if command storage cannot grow.
    pub fn fill(&mut self, rect: Rect, color: Rgb, source: Source) -> Result<(), Error> {
        self.append(Command::Fill {
            rect,
            color,
            source,
        })
    }

    /// # Errors
    /// Fails for invalid coverage, the glyph or command budget, or allocation.
    pub fn glyph_run(
        &mut self,
        bounds: Rect,
        coverage: Vec<u8>,
        color: Rgb,
        source: Source,
    ) -> Result<(), Error> {
        let len = usize::try_from(bounds.width)
            .ok()
            .and_then(|w| {
                usize::try_from(bounds.height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            })
            .ok_or(Error::InvalidCoverage)?;
        if len != coverage.len() {
            return Err(Error::InvalidCoverage);
        }
        let new_total = self
            .glyph_bytes
            .checked_add(len)
            .ok_or(Error::GlyphBudget)?;
        if new_total > MAX_GLYPH_BYTES {
            return Err(Error::GlyphBudget);
        }
        self.append(Command::GlyphRun {
            bounds,
            coverage,
            color,
            source,
        })?;
        self.glyph_bytes = new_total;
        Ok(())
    }

    /// # Errors
    /// Fails at the clip/command limit or if command storage cannot grow.
    pub fn push_clip(&mut self, rect: Rect) -> Result<(), Error> {
        if self.clip_depth >= MAX_CLIP_DEPTH {
            return Err(Error::TooManyClips);
        }
        self.append(Command::PushClip(rect))?;
        self.clip_depth += 1;
        Ok(())
    }

    /// # Errors
    /// Fails on a missing clip or if command storage cannot grow.
    pub fn pop_clip(&mut self) -> Result<(), Error> {
        if self.clip_depth == 0 {
            return Err(Error::UnbalancedClip);
        }
        self.append(Command::PopClip)?;
        self.clip_depth -= 1;
        Ok(())
    }

    /// Append a complete owned paint list after existing commands. A list
    /// with open clips cannot cross this boundary: the caller must finish
    /// background and text painting before combining them. Capacity and
    /// glyph budgets are checked before either list changes.
    ///
    /// # Errors
    /// Refuses open clips, command/glyph limits, or failed allocation without
    /// modifying the destination list.
    pub fn append_list(&mut self, mut other: Self) -> Result<(), Error> {
        if self.clip_depth != 0 || other.clip_depth != 0 {
            return Err(Error::UnbalancedClip);
        }
        let command_count = self
            .commands
            .len()
            .checked_add(other.commands.len())
            .ok_or(Error::TooManyCommands)?;
        if command_count > MAX_COMMANDS {
            return Err(Error::TooManyCommands);
        }
        let bytes = self
            .glyph_bytes
            .checked_add(other.glyph_bytes)
            .ok_or(Error::GlyphBudget)?;
        if bytes > MAX_GLYPH_BYTES {
            return Err(Error::GlyphBudget);
        }
        self.commands
            .try_reserve(other.commands.len())
            .map_err(|_| Error::Allocation)?;
        self.commands.append(&mut other.commands);
        self.glyph_bytes = bytes;
        Ok(())
    }

    /// Paint into an opaque, row-major RGBA surface. Returns an error rather
    /// than painting an open clip stack or allocating an unbounded surface.
    /// Only the resulting surface is allocated during rasterization.
    ///
    /// # Errors
    /// Fails on an open clip stack, oversized surface, or allocation.
    pub fn rasterize(&self, width: u32, height: u32, background: Rgb) -> Result<Vec<u8>, Error> {
        if self.clip_depth != 0 {
            return Err(Error::UnbalancedClip);
        }
        let pixels = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .ok_or(Error::InvalidSurface)?;
        if pixels > MAX_PIXELS {
            return Err(Error::InvalidSurface);
        }
        let bytes = pixels.checked_mul(4).ok_or(Error::InvalidSurface)?;
        let mut out = Vec::new();
        out.try_reserve_exact(bytes)
            .map_err(|_| Error::Allocation)?;
        for _ in 0..pixels {
            out.extend_from_slice(&[background.0, background.1, background.2, 255]);
        }
        let full = Area {
            x0: 0,
            y0: 0,
            x1: i64::from(width),
            y1: i64::from(height),
        };
        let mut clips = [full; MAX_CLIP_DEPTH + 1];
        let mut depth = 0;
        for command in &self.commands {
            match command {
                Command::PushClip(rect) => {
                    depth += 1;
                    clips[depth] = clips[depth - 1].intersect(Area::from(*rect));
                }
                Command::PopClip => depth -= 1,
                Command::Fill { rect, color, .. } => {
                    paint(&mut out, width, clips[depth], *rect, *color, None);
                }
                Command::GlyphRun {
                    bounds,
                    coverage,
                    color,
                    ..
                } => {
                    paint(
                        &mut out,
                        width,
                        clips[depth],
                        *bounds,
                        *color,
                        Some(coverage),
                    );
                }
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Copy)]
struct Area {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}
impl Area {
    fn from(r: Rect) -> Self {
        Self {
            x0: i64::from(r.x),
            y0: i64::from(r.y),
            x1: i64::from(r.x) + i64::from(r.width),
            y1: i64::from(r.y) + i64::from(r.height),
        }
    }
    fn intersect(self, other: Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }
}

fn paint(
    out: &mut [u8],
    surface_width: u32,
    clip: Area,
    rect: Rect,
    color: Rgb,
    coverage: Option<&[u8]>,
) {
    let visible = clip.intersect(Area::from(rect));
    if visible.x0 >= visible.x1 || visible.y0 >= visible.y1 {
        return;
    }
    let src = [color.0, color.1, color.2];
    let stride = surface_width as usize;
    let mask_stride = rect.width as usize;
    for y in visible.y0..visible.y1 {
        for x in visible.x0..visible.x1 {
            let alpha = coverage.map_or(255, |mask| {
                let my = usize::try_from(y - i64::from(rect.y)).expect("clipped mask row");
                let mx = usize::try_from(x - i64::from(rect.x)).expect("clipped mask column");
                mask[my * mask_stride + mx]
            });
            let row = usize::try_from(y).expect("clipped surface row");
            let col = usize::try_from(x).expect("clipped surface column");
            let index = (row * stride + col) * 4;
            for c in 0..3 {
                let mixed = u32::from(src[c]) * u32::from(alpha)
                    + u32::from(out[index + c]) * u32::from(255 - alpha);
                out[index + c] = u8::try_from((mixed + 127) / 255).expect("bounded channel");
            }
        }
    }
}
