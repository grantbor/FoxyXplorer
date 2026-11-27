use iced::widget::canvas::{self, Geometry};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use crate::{App, Message};

pub struct DemProgram<'a> { pub app: &'a App }
pub struct DemLegendProgram<'a> { pub app: &'a App }

impl<'a> canvas::Program<Message> for DemProgram<'a> {
    type State = ();

    fn draw(
        &self, _: &Self::State, renderer: &Renderer, _: &Theme,
        bounds: Rectangle, _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let view = self.app.current_dem_view();
        if let Some(overlay) = self.app.dem.visible(view) {
            frame.draw_image(
                Rectangle { x: 0.0, y: 0.0, width: bounds.width, height: bounds.height },
                canvas::Image::new(overlay.image.clone()),
            );
        }
        vec![frame.into_geometry()]
    }
}

// Give the legend all available vertical space while reserving a bottom
// strip for one or two map attributions, even on short viewports.
const LEGEND_TOP: f32 = 24.0;
const LEGEND_BOTTOM_RESERVE: f32 = 74.0;
const LEGEND_WIDTH: f32 = 59.0;
const LEGEND_BAR_WIDTH: f32 = 13.0;
const MIN_LABEL_GAP: f32 = 26.0;

fn legend_height(window_height: f32) -> Option<f32> {
    if !window_height.is_finite() { return None; }
    let height = window_height - LEGEND_TOP - LEGEND_BOTTOM_RESERVE;
    (height >= 55.0).then_some(height)
}

// Round the desired metre interval upward to a cartographic 1-2-2.5-5-10 step.
fn nice_tick_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 { return 1.0; }
    let magnitude = 10.0_f64.powf(raw.log10().floor());
    for multiple in [1.0, 2.0, 2.5, 5.0, 10.0] {
        let step = multiple * magnitude;
        if step >= raw { return step; }
    }
    10.0 * magnitude
}

/// Pairs of (elevation, vertical fraction), high at 0 and low at 1.
/// The two percentile endpoints always appear; intermediate values are rounded
/// to convenient elevation intervals, with room for all labels.
fn legend_ticks(lower: f32, upper: f32, bar_height: f32) -> Vec<(f32, f32)> {
    if !lower.is_finite() || !upper.is_finite() || upper <= lower || bar_height <= 0.0 {
        return Vec::new();
    }
    let low = lower as f64;
    let high = upper as f64;
    let span = high - low;
    let desired_intervals = (bar_height / 52.0).max(1.0) as f64;
    let step = nice_tick_step(span / desired_intervals);
    let first = (low / step).ceil() as i64;
    let last = (high / step).floor() as i64;
    let mut ticks = vec![(upper, 0.0)];
    // The number of marks is bounded by the viewport height; the guard also
    // protects against unexpected DEM values or floating-point overflow.
    if last >= first && last - first < 1000 {
        for index in (first..=last).rev() {
            let elevation = index as f64 * step;
            let fraction = ((high - elevation) / span) as f32;
            let distance = fraction * bar_height;
            if distance >= MIN_LABEL_GAP && bar_height - distance >= MIN_LABEL_GAP {
                ticks.push((elevation as f32, fraction));
            }
        }
    }
    ticks.push((lower, 1.0));
    ticks
}

fn elevation_label(value: f32, span: f32) -> String {
    if span < 10.0 { format!("{value:.1}") } else { format!("{value:.0}") }
}

impl<'a> canvas::Program<Message> for DemLegendProgram<'a> {
    type State = ();

    fn draw(
        &self, _: &Self::State, renderer: &Renderer, _: &Theme,
        bounds: Rectangle, _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let view = self.app.current_dem_view();
        if let (Some(overlay), Some(height)) =
            (self.app.dem.visible(view), legend_height(bounds.height))
        {
            let bg_left = (bounds.width - LEGEND_WIDTH - 5.0).max(0.0);
            let bg_width = (bounds.width - bg_left - 5.0).min(LEGEND_WIDTH);
            let bar_left = bg_left + 6.0;
            let bar_top = LEGEND_TOP;
            frame.fill_rectangle(
                Point::new(bg_left, bar_top - 10.0),
                Size::new(bg_width, height + 20.0),
                Color::from_rgba(0.05, 0.06, 0.08, 0.78),
            );

            // Palette and limits are identical to those used by DEM rendering.
            let steps = height.ceil() as u32;
            for i in 0..steps {
                let t = 1.0 - i as f32 / (steps.saturating_sub(1).max(1)) as f32;
                let [r, g, b] = crate::dem::palette(t);
                frame.fill_rectangle(
                    Point::new(bar_left, bar_top + i as f32),
                    Size::new(LEGEND_BAR_WIDTH, 1.0),
                    Color::from_rgb8(r, g, b),
                );
            }

            for (elevation, fraction) in legend_ticks(overlay.lower, overlay.upper, height) {
                let y = bar_top + height * fraction;
                frame.fill_rectangle(
                    Point::new(bar_left + LEGEND_BAR_WIDTH, y),
                    Size::new(4.0, 1.0),
                    Color::WHITE,
                );
                frame.fill_text(canvas::Text {
                    content: elevation_label(elevation, overlay.upper - overlay.lower),
                    position: Point::new(bar_left + LEGEND_BAR_WIDTH + 5.0, y),
                    size: iced::Pixels(11.0),
                    color: Color::WHITE,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::{elevation_label, legend_height, legend_ticks, nice_tick_step};

    #[test]
    fn legend_grows_with_viewport_instead_of_stopping_at_260_pixels() {
        let small = legend_height(360.0).unwrap();
        let large = legend_height(1080.0).unwrap();
        assert!(small >= 200.0 && small < large);
        assert_eq!(large, 1080.0 - 24.0 - 74.0);
        assert_eq!(legend_height(800.0), Some(702.0));
        // The legend background ends before the upper of two attribution rows.
        assert!(24.0 + large + 10.0 < 1080.0 - 46.0);
        assert_eq!(legend_height(100.0), None);
    }

    #[test]
    fn legend_ticks_get_denser_with_more_vertical_space() {
        let small = legend_ticks(590.0, 2750.0, 220.0);
        let large = legend_ticks(590.0, 2750.0, 720.0);
        assert!(large.len() > small.len());
        assert_eq!(small.first(), Some(&(2750.0, 0.0)));
        assert_eq!(small.last(), Some(&(590.0, 1.0)));
        assert_eq!(large.first(), Some(&(2750.0, 0.0)));
        assert_eq!(large.last(), Some(&(590.0, 1.0)));
        assert!(large.windows(2).all(|pair| pair[0].1 < pair[1].1));
        assert_eq!(nice_tick_step(112.0), 200.0);
    }

    #[test]
    fn legend_handles_narrow_elevation_ranges() {
        let ticks = legend_ticks(501.4, 503.8, 600.0);
        assert!(ticks.len() >= 2);
        assert_eq!(elevation_label(ticks[0].0, 2.4), "503.8");
        assert!(legend_ticks(f32::NAN, 100.0, 200.0).is_empty());
        assert!(legend_ticks(500.0, 500.0, 200.0).is_empty());
    }
}
