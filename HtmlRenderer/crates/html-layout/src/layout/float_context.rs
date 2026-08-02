#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FloatBand {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub side: FloatSide,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FloatContext {
    bands: Vec<FloatBand>,
}

impl FloatContext {
    pub(crate) fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    pub(crate) fn max_bottom(&self) -> f64 {
        self.bands.iter().map(|band| band.bottom).fold(0.0, f64::max)
    }

    pub(crate) fn add_band(&mut self, band: FloatBand) {
        if band.right > band.left && band.bottom > band.top {
            self.bands.push(band);
        }
    }

    pub(crate) fn available(&self, y: f64, height: f64, container_left: f64, container_right: f64) -> (f64, f64) {
        let bottom = y + height.max(0.0);
        let mut left = container_left;
        let mut right = container_right;
        for band in &self.bands {
            if band.bottom <= y || band.top >= bottom {
                continue;
            }
            if band.left <= container_left + f64::EPSILON {
                left = left.max(band.right);
            }
            if band.right >= container_right - f64::EPSILON {
                right = right.min(band.left);
            }
        }
        (left.min(container_right), right.max(container_left))
    }

    pub(crate) fn clear_to(&self, y: f64, side: ClearSide) -> f64 {
        let mut clear_y = y;
        for band in &self.bands {
            let affects_side = match side {
                ClearSide::Left => matches!(band.side, FloatSide::Left),
                ClearSide::Right => matches!(band.side, FloatSide::Right),
                ClearSide::Both => true,
            };
            if affects_side {
                clear_y = clear_y.max(band.bottom);
            }
        }
        clear_y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClearSide {
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FloatSide {
    #[default]
    Left,
    Right,
}
