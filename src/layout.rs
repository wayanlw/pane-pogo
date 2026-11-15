use crate::config::{MonitorRole, Order};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// How a pair action arranges the current and the other window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    /// Side by side on one monitor. `left_share` is the width fraction of the left part.
    Split { left_share: f32 },
    /// Each window fills a different monitor.
    Across,
}

/// Where the current window and the other window end up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub current: Rect,
    pub other: Rect,
}

/// Places the current and other window.
///
/// - `Split`: the chosen monitor is split. The current window takes the `order` side.
///   With no secondary monitor, the primary is used.
/// - `Across`: the current window fills the chosen monitor, the other fills the remaining one.
///   With no secondary monitor, falls back to a half split on the primary.
pub fn pair(
    layout: Layout,
    primary: Rect,
    secondary: Option<Rect>,
    order: Order,
    monitor: MonitorRole,
) -> Pair {
    match (layout, secondary) {
        (Layout::Across, Some(sec)) => match monitor {
            MonitorRole::Primary => Pair {
                current: primary,
                other: sec,
            },
            MonitorRole::Secondary => Pair {
                current: sec,
                other: primary,
            },
        },
        (Layout::Across, None) => split(primary, order, 0.5),
        (Layout::Split { left_share }, _) => {
            let area = match (monitor, secondary) {
                (MonitorRole::Secondary, Some(sec)) => sec,
                _ => primary,
            };
            split(area, order, left_share)
        }
    }
}

/// Rect for the current window when no other window is available.
/// Same rules as `pair`, so the current window keeps its usual place.
pub fn solo(
    layout: Layout,
    primary: Rect,
    secondary: Option<Rect>,
    order: Order,
    monitor: MonitorRole,
) -> Rect {
    pair(layout, primary, secondary, order, monitor).current
}

fn split(area: Rect, order: Order, left_share: f32) -> Pair {
    let left_w = (area.w as f32 * left_share).round() as i32;
    let left = Rect {
        x: area.x,
        y: area.y,
        w: left_w,
        h: area.h,
    };
    let right = Rect {
        x: area.x + left_w,
        y: area.y,
        w: area.w - left_w,
        h: area.h,
    };
    match order {
        Order::CurrentLeft => Pair {
            current: left,
            other: right,
        },
        Order::CurrentRight => Pair {
            current: right,
            other: left,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1040,
    };
    const SECONDARY: Rect = Rect {
        x: 1920,
        y: 0,
        w: 2560,
        h: 1400,
    };
    const HALF: Layout = Layout::Split { left_share: 0.5 };
    const TWO_THIRDS_LEFT: Layout = Layout::Split {
        left_share: 2.0 / 3.0,
    };
    const ONE_THIRD_LEFT: Layout = Layout::Split {
        left_share: 1.0 / 3.0,
    };

    #[test]
    fn halves_current_right() {
        let p = pair(
            HALF,
            PRIMARY,
            Some(SECONDARY),
            Order::CurrentRight,
            MonitorRole::Primary,
        );
        assert_eq!(
            p.other,
            Rect {
                x: 0,
                y: 0,
                w: 960,
                h: 1040
            }
        );
        assert_eq!(
            p.current,
            Rect {
                x: 960,
                y: 0,
                w: 960,
                h: 1040
            }
        );
    }

    #[test]
    fn two_thirds_left_is_positional() {
        // Current on the left gets the 2/3; current on the right gets the 1/3.
        let l = pair(
            TWO_THIRDS_LEFT,
            PRIMARY,
            None,
            Order::CurrentLeft,
            MonitorRole::Primary,
        );
        assert_eq!(
            l.current,
            Rect {
                x: 0,
                y: 0,
                w: 1280,
                h: 1040
            }
        );
        assert_eq!(
            l.other,
            Rect {
                x: 1280,
                y: 0,
                w: 640,
                h: 1040
            }
        );
        let r = pair(
            TWO_THIRDS_LEFT,
            PRIMARY,
            None,
            Order::CurrentRight,
            MonitorRole::Primary,
        );
        assert_eq!(
            r.current,
            Rect {
                x: 1280,
                y: 0,
                w: 640,
                h: 1040
            }
        );
        assert_eq!(
            r.other,
            Rect {
                x: 0,
                y: 0,
                w: 1280,
                h: 1040
            }
        );
    }

    #[test]
    fn one_third_left_is_positional() {
        let r = pair(
            ONE_THIRD_LEFT,
            PRIMARY,
            None,
            Order::CurrentRight,
            MonitorRole::Primary,
        );
        assert_eq!(
            r.current,
            Rect {
                x: 640,
                y: 0,
                w: 1280,
                h: 1040
            }
        );
        assert_eq!(
            r.other,
            Rect {
                x: 0,
                y: 0,
                w: 640,
                h: 1040
            }
        );
    }

    #[test]
    fn split_on_secondary_uses_secondary_area() {
        let p = pair(
            HALF,
            PRIMARY,
            Some(SECONDARY),
            Order::CurrentLeft,
            MonitorRole::Secondary,
        );
        assert_eq!(
            p.current,
            Rect {
                x: 1920,
                y: 0,
                w: 1280,
                h: 1400
            }
        );
        assert_eq!(
            p.other,
            Rect {
                x: 3200,
                y: 0,
                w: 1280,
                h: 1400
            }
        );
    }

    #[test]
    fn split_on_missing_secondary_uses_primary() {
        let p = pair(
            HALF,
            PRIMARY,
            None,
            Order::CurrentRight,
            MonitorRole::Secondary,
        );
        assert_eq!(p.current.x, 960);
    }

    #[test]
    fn across_puts_current_on_chosen_monitor() {
        let s = pair(
            Layout::Across,
            PRIMARY,
            Some(SECONDARY),
            Order::CurrentRight,
            MonitorRole::Secondary,
        );
        assert_eq!((s.current, s.other), (SECONDARY, PRIMARY));
        let p = pair(
            Layout::Across,
            PRIMARY,
            Some(SECONDARY),
            Order::CurrentRight,
            MonitorRole::Primary,
        );
        assert_eq!((p.current, p.other), (PRIMARY, SECONDARY));
    }

    #[test]
    fn across_without_secondary_falls_back_to_half_split() {
        let p = pair(
            Layout::Across,
            PRIMARY,
            None,
            Order::CurrentLeft,
            MonitorRole::Secondary,
        );
        assert_eq!(
            p.current,
            Rect {
                x: 0,
                y: 0,
                w: 960,
                h: 1040
            }
        );
    }

    #[test]
    fn solo_takes_current_place_only() {
        let r = solo(
            TWO_THIRDS_LEFT,
            PRIMARY,
            None,
            Order::CurrentRight,
            MonitorRole::Primary,
        );
        assert_eq!(
            r,
            Rect {
                x: 1280,
                y: 0,
                w: 640,
                h: 1040
            }
        );
    }
}
