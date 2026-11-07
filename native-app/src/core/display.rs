//! Physical display identity and virtual-desktop geometry.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PhysicalBounds {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Result<Self, &'static str> {
        if right <= left || bottom <= top {
            return Err("display bounds must have positive width and height");
        }
        Ok(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    fn validate(self) -> Result<(), &'static str> {
        if self.right <= self.left || self.bottom <= self.top {
            return Err("display bounds must have positive width and height");
        }
        Ok(())
    }

    pub fn width(self) -> u32 {
        (i64::from(self.right) - i64::from(self.left)) as u32
    }

    pub fn height(self) -> u32 {
        (i64::from(self.bottom) - i64::from(self.top)) as u32
    }

    pub fn contains(self, point: PhysicalPoint) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    fn distance_squared_to(self, point: PhysicalPoint) -> i128 {
        let dx = if point.x < self.left {
            i128::from(self.left) - i128::from(point.x)
        } else if point.x >= self.right {
            i128::from(point.x) - i128::from(self.right - 1)
        } else {
            0
        };
        let dy = if point.y < self.top {
            i128::from(self.top) - i128::from(point.y)
        } else if point.y >= self.bottom {
            i128::from(point.y) - i128::from(self.bottom - 1)
        } else {
            0
        };
        dx * dx + dy * dy
    }
}

/// A display device name is stable for the current Windows display topology.
/// It is deliberately not persisted because Windows may rename displays when
/// devices or their arrangement change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayMetadata {
    pub id: String,
    pub bounds: PhysicalBounds,
    pub effective_dpi: u32,
    pub is_primary: bool,
}

impl DisplayMetadata {
    pub fn new(
        id: String,
        bounds: PhysicalBounds,
        effective_dpi: u32,
        is_primary: bool,
    ) -> Result<Self, &'static str> {
        bounds.validate()?;
        if id.trim().is_empty() {
            return Err("display identifier must not be empty");
        }
        if effective_dpi == 0 {
            return Err("display DPI must be positive");
        }
        Ok(Self {
            id,
            bounds,
            effective_dpi,
            is_primary,
        })
    }

    pub fn scale_factor(&self) -> f64 {
        f64::from(self.effective_dpi) / 96.0
    }

    /// Map a monitor-local pixel edge to its signed virtual-desktop position.
    pub fn local_to_desktop(&self, x: u32, y: u32) -> Option<PhysicalPoint> {
        if x > self.bounds.width() || y > self.bounds.height() {
            return None;
        }
        Some(PhysicalPoint {
            x: i32::try_from(i64::from(self.bounds.left) + i64::from(x)).ok()?,
            y: i32::try_from(i64::from(self.bounds.top) + i64::from(y)).ok()?,
        })
    }
}

/// Resolve an explicit display ID, or use the primary display when no ID was
/// selected. An unavailable explicit selection is an error, not a silent
/// switch to a different display.
pub fn select_display<'a>(
    displays: &'a [DisplayMetadata],
    requested_id: Option<&str>,
) -> Result<&'a DisplayMetadata, String> {
    if let Some(requested_id) = requested_id {
        return displays
            .iter()
            .find(|display| display.id == requested_id)
            .ok_or_else(|| format!("Selected display '{requested_id}' is no longer available"));
    }

    displays
        .iter()
        .filter(|display| display.is_primary)
        .min_by(|left, right| left.id.cmp(&right.id))
        .ok_or_else(|| "Windows did not report a primary display".to_owned())
}

/// Select the display containing a physical virtual-desktop cursor position.
/// If Windows reports a cursor point in a gap between displays, choose the
/// nearest display deterministically rather than silently using the primary.
pub fn select_display_at_point<'a>(
    displays: &'a [DisplayMetadata],
    point: PhysicalPoint,
) -> Result<&'a DisplayMetadata, String> {
    displays
        .iter()
        .find(|display| display.bounds.contains(point))
        .or_else(|| {
            displays.iter().min_by_key(|display| {
                (
                    display.bounds.distance_squared_to(point),
                    !display.is_primary,
                    display.id.as_str(),
                )
            })
        })
        .ok_or_else(|| "Windows did not report any connected displays".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        select_display, select_display_at_point, DisplayMetadata, PhysicalBounds, PhysicalPoint,
    };

    fn display(id: &str, bounds: PhysicalBounds, is_primary: bool) -> DisplayMetadata {
        DisplayMetadata::new(id.into(), bounds, 120, is_primary).unwrap()
    }

    #[test]
    fn bounds_preserve_zero_positive_and_negative_virtual_origins() {
        let at_origin = PhysicalBounds::new(0, 0, 1920, 1080).unwrap();
        let right_and_below = PhysicalBounds::new(1920, 1080, 3200, 1800).unwrap();
        let left_and_above = PhysicalBounds::new(-1920, -1080, 0, 0).unwrap();

        assert_eq!((at_origin.width(), at_origin.height()), (1920, 1080));
        assert_eq!((right_and_below.left, right_and_below.top), (1920, 1080));
        assert_eq!((left_and_above.left, left_and_above.top), (-1920, -1080));
        assert_eq!(
            (left_and_above.width(), left_and_above.height()),
            (1920, 1080)
        );
        assert!(PhysicalBounds::new(0, 0, 0, 1080).is_err());
    }

    #[test]
    fn defaults_to_primary_and_resolves_an_explicit_display_id() {
        let displays = [
            display(
                "DISPLAY2",
                PhysicalBounds::new(-1280, 0, 0, 1024).unwrap(),
                false,
            ),
            display(
                "DISPLAY1",
                PhysicalBounds::new(0, 0, 1920, 1080).unwrap(),
                true,
            ),
        ];

        assert_eq!(select_display(&displays, None).unwrap().id, "DISPLAY1");
        assert_eq!(
            select_display(&displays, None).unwrap().scale_factor(),
            1.25
        );
        assert_eq!(
            select_display(&displays, Some("DISPLAY2")).unwrap().id,
            "DISPLAY2"
        );
        assert!(select_display(&displays, Some("DISPLAY3")).is_err());
    }

    #[test]
    fn maps_monitor_local_pixels_to_signed_desktop_coordinates() {
        let display = display(
            "DISPLAY2",
            PhysicalBounds::new(-1920, 0, 0, 1080).unwrap(),
            false,
        );

        assert_eq!(
            display.local_to_desktop(320, 240),
            Some(PhysicalPoint { x: -1600, y: 240 })
        );
        assert_eq!(
            display.local_to_desktop(1920, 1080),
            Some(PhysicalPoint { x: 0, y: 1080 })
        );
        assert_eq!(display.local_to_desktop(1921, 0), None);
    }

    #[test]
    fn cursor_selects_the_containing_monitor_with_negative_and_above_origins() {
        let displays = [
            DisplayMetadata::new(
                "PRIMARY".into(),
                PhysicalBounds::new(0, 0, 1920, 1080).unwrap(),
                96,
                true,
            )
            .unwrap(),
            DisplayMetadata::new(
                "LEFT_ABOVE".into(),
                PhysicalBounds::new(-1600, -900, 0, 0).unwrap(),
                144,
                false,
            )
            .unwrap(),
        ];

        let selected =
            select_display_at_point(&displays, PhysicalPoint { x: -800, y: -450 }).unwrap();
        assert_eq!(selected.id, "LEFT_ABOVE");
        assert_eq!(selected.scale_factor(), 1.5);
        assert_eq!(
            select_display_at_point(&displays, PhysicalPoint { x: 1200, y: 800 })
                .unwrap()
                .id,
            "PRIMARY"
        );
        assert_eq!(
            select_display_at_point(&[], PhysicalPoint { x: -1, y: 0 }).unwrap_err(),
            "Windows did not report any connected displays"
        );
    }

    #[test]
    fn cursor_in_virtual_desktop_gap_uses_the_nearest_monitor_not_always_primary() {
        let displays = [
            display(
                "PRIMARY",
                PhysicalBounds::new(0, 0, 1920, 1080).unwrap(),
                true,
            ),
            display(
                "LEFT",
                PhysicalBounds::new(-1920, 0, 0, 1080).unwrap(),
                false,
            ),
        ];

        assert_eq!(
            select_display_at_point(&displays, PhysicalPoint { x: -1900, y: 1200 })
                .unwrap()
                .id,
            "LEFT"
        );
    }
}
