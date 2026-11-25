//! Прозрачный неинтерактивный слой WPT между подложкой карты и треками.
use iced::widget::canvas::{self, Geometry};
use iced::{Point, Rectangle, Renderer, Theme};
use crate::util::geo::merc_of;
use crate::{App, Message};

pub struct WikiProgram<'a> { pub app: &'a App }

impl<'a> canvas::Program<Message> for WikiProgram<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let app = self.app;
        let geom = app.wiki_cache.draw(renderer, bounds.size(), |frame| {
            if !app.show_wiki { return; }
            let half_w = bounds.width as f64 / 2.0;
            let half_h = bounds.height as f64 / 2.0;
            for wiki in &app.wiki_points {
                let (mx, my) = merc_of(wiki.lon, wiki.lat);
                let pt = Point::new(
                    (half_w + (mx - app.cx) / app.scale) as f32,
                    (half_h - (my - app.cy) / app.scale) as f32,
                );
                if pt.x < -200.0 || pt.x > bounds.width + 200.0
                    || pt.y < -60.0 || pt.y > bounds.height + 60.0 {
                    continue;
                }
                super::waypoint_label::draw_waypoint_label(
                    frame, pt, &wiki.name, [0xcc; 3], [0x00; 3], false,
                );
            }
        });
        vec![geom]
    }
}
