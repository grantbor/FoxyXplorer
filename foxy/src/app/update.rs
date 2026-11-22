use std::time::{Duration, Instant};
use std::path::PathBuf;
use iced::mouse::ScrollDelta;
use iced::{Point, Task};
use coord::CoordFormat;

use crate::{App, Message, Modal, DrawingTrack, Selection, ContextMenuState, ContextMenuItem,
    DEFAULT_SCALE,
};
use crate::domain;
use crate::io;
use crate::session;
use crate::util::coords::{build_template, date_offset, parse_coords};
use crate::util::geo::lonlat_of;
use crate::{load_kit, save_session};

use std::sync::Arc;
use crate::UndoState;
use crate::{AppTheme, theme_pair_for};

use std::collections::HashSet; 
use crate::{TrackRowData, TrackListState}; 
use crate::util::geo::merc_of; 
use crate::util::geo::dist_to_segment;
use crate::t;
use crate::SettingsDraft;

fn is_space(key: &iced::keyboard::Key) -> bool {
    matches!(key, iced::keyboard::Key::Named(iced::keyboard::key::Named::Space))
        || matches!(key, iced::keyboard::Key::Character(c) if c == " ")
}

impl App {
    pub(crate) fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            // --- View & Canvas ---
            Message::MapPressed(_) | Message::MapReleased(_) | Message::Moved(_) | Message::Scrolled(_)
            | Message::Fit | Message::WindowResized(_) | Message::WindowMoved(_)
            | Message::DividerPressed | Message::DividerReleased | Message::CursorMoved(_)  => {
                self.update_view(m);
            }

            // --- UI (Верхняя панель) ---
            Message::ToggleGrid | Message::ToggleLabels | Message::CycleFormat 
            | Message::SearchTracksChanged(_) | Message::SearchWaypointsChanged(_) 
            | Message::ModalDragStart | Message::SelectTool(_) 
            | Message::KeyPressed(_, _) | Message::KeyReleased(_) 
            | Message::SaveSessionAs | Message::SaveSessionAsPicked(_)
            | Message::LoadSessionPrompt | Message::LoadSessionNoPrompt
            | Message::LoadSessionPicked(_) 
            | Message::SaveAndLoadSession | Message::SaveAndLoadSessionSaved(_) 
            | Message::ConfirmClearAll | Message::ExecuteClearAll | Message::CancelClearAll | Message::GlobalCursorMoved(_) 
            | Message::ShowContextMenu(_, _) | Message::ContextMenuClose | Message::ContextMenuSelect(_) 
            | Message::CopyCoordinates(_, _) | Message::ToggleTheme | Message::RefreshSystemTheme 
            | Message::OpenAbout | Message::CloseAbout 
            | Message::OpenSettings | Message::CloseSettings | Message::ApplySettings
            | Message::SettingsCoordChanged(_)
            | Message::SettingsLangChanged(_) | Message::SettingsToleranceChanged(_)
            | Message::SettingsGridColorChanged(_) | Message::SettingsThemeChanged(_)
            | Message::SettingsChooseDropFolder | Message::SettingsDropFolderPicked(_)
            | Message::OpenFoxyDrop | Message::CloseFoxyDrop
            | Message::FoxyDropStart | Message::FoxyDropStop
            | Message::FoxyDropIpSelected(_) | Message::FoxyDropStarted(_)
            | Message::FoxyDropStopped | Message::FoxyDropHeartbeat | Message::FoxyDropCopyUrl
            | Message::OpenUrl(_) => {
                return self.update_ui(m); 
            }

            // --- Треки ---
            Message::Layer(_) | Message::SelectTrack(_) | Message::ToggleTrackVisible(_, _)
            | Message::ToggleAllTracksVisible | Message::DeleteSelectedTracks
            | Message::OpenTrackProps(_) | Message::ApplyTrackProps | Message::CloseTrackProps
            | Message::ToggleTrackSaveMenu | Message::SimplifyTracks |  Message::FitTrackToScreen | Message::MergeTracks
            | Message::DeleteSelectedPoints | Message::FreehandReleased 
            | Message::DeleteTrackById(_) | Message::FitTrackById(_) | Message::SplitTrackBySegments |  Message::SplitTrackAt(_, _, _)  => {
                self.update_tracks(m);
            }

            // --- Модальное окно трека ---
            Message::DraftDescChanged(_) | Message::DraftDateChanged(_) | Message::DraftCallsignChanged(_)
            | Message::DraftNum1Changed(_) | Message::DraftNum2Changed(_) | Message::DraftSuffixChanged(_)
            | Message::DraftDateToday | Message::DraftDateYesterday 
            | Message::DraftColorChanged(_) | Message::DraftFillColorChanged(_) | Message::DraftWidthChanged(_)
            | Message::DraftStyleChanged(_) | Message::DraftTypeChanged(_) | Message::DraftFillTypeChanged(_) 
            | Message::ToggleColorPicker(_)  => {
                self.update_track_modal(m);
            }

            // --- Точки ---
            Message::SelectWaypoint(_) | Message::ToggleWaypointVisible(_, _)
            | Message::ToggleAllWaypointsVisible | Message::DeleteSelectedWaypoints
            | Message::OpenWaypointProps(_) | Message::AddNewWaypoint | Message::ApplyWaypointProps
            | Message::CloseWaypointProps | Message::ToggleWpSaveMenu | Message::CenterOnWaypoint(_)
            | Message::ClearWaypointCrosshair | Message::MeasureWaypoints | Message::CloseWaypointMeasure 
            | Message::DeleteWaypointById(_) | Message::AddWaypointAt(_, _) |  Message::OpenWaypointProjection(_) 
            | Message::CloseWaypointProjection | Message::ProjectAzimuthChanged(_) | Message::ProjectDistanceChanged(_) 
            | Message::ExecuteProjection  => {
                return self.update_waypoints(m);
            }

            // --- Модальное окно точки ---
            Message::DraftWpNameChanged(_) | Message::DraftWpDescChanged(_) | Message::DraftWpCoordsChanged(_)
            | Message::DraftWpEleChanged(_) | Message::DraftWpTextColorChanged(_) | Message::DraftWpBgColorChanged(_) => {
                self.update_wp_modal(m);
            }

            // --- Сохранение файлов ---
            Message::SaveTracksPlt | Message::SaveTracksPltPicked(_)
            | Message::SaveTracksGpx | Message::SaveTracksGpxPicked(_)
            | Message::SaveTracksMs | Message::SaveTracksMsPicked(_)
            | Message::SaveTracksKml | Message::SaveTracksKmlPicked(_)
            | Message::SaveWaypointsWptNew | Message::SaveWaypointsWptNewPicked(_)
            | Message::SaveWaypointsWptAppend | Message::SaveWaypointsWptAppendPicked(_)
            | Message::SaveTrackProps | Message::SaveTrackPropsPicked(_) => {
                return self.update_save(m);
            }

            // --- Система и прочее ---
            Message::PickKit | Message::KitPicked(_) | Message::PickTrack | Message::TracksPicked(_)
            | Message::WindowCloseRequested | Message::ModifiersChanged(_) | Message::OsmTileLoaded(..)
            | Message::AutoSaveTick | Message::None => {
                return self.update_system(m);
            }
            
            // --- Tracklist ----
            Message::OpenTrackList(_) | Message::CloseTrackList | Message::SelectTrackPoint(_, _)
            | Message::DeleteSelectedTrackPoints
            | Message::CenterOnTrackPoint(_, _) | Message::SplitTrackAtPoint(_, _) | Message::TrackListUp | Message::TrackListDown
            | Message::SelectPointsOutsideGrid | Message::StitchSegments => {
                return self.update_track_list(m);
            }
        }

        self.request_osm_tiles()
    }
    
    fn perform_undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(UndoState {
                tracks: self.tracks.clone(),
                waypoints: self.waypoints.clone(),
                next_id: self.next_id,
                label: prev.label.clone(),
            });
            self.tracks = prev.tracks;
            self.waypoints = prev.waypoints;
            self.next_id = prev.next_id;
            self.tracks_cache.clear();
            self.status = format!("Отменено: {}", prev.label);
        }
    }

    fn perform_redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(UndoState {
                tracks: self.tracks.clone(),
                waypoints: self.waypoints.clone(),
                next_id: self.next_id,
                label: next.label.clone(),
            });
            self.tracks = next.tracks;
            self.waypoints = next.waypoints;
            self.next_id = next.next_id;
            self.tracks_cache.clear();
            self.status = format!("Повторено: {}", next.label);
        }
    }
    
    pub(crate) fn push_undo(&mut self, label: &str) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(UndoState {
            tracks: self.tracks.clone(),
            waypoints: self.waypoints.clone(),
            next_id: self.next_id,
            label: label.to_string(),
        });
        self.redo_stack.clear();
    }
    fn apply_session(&mut self, sess: session::Session) {
        self.tracks = sess.tracks.into_iter().map(Arc::new).collect();
        self.waypoints = sess.waypoints.into_iter().map(Arc::new).collect();
        self.next_id = sess.next_id;
        for t in &mut self.tracks {
            domain::recompute_track_stats(Arc::make_mut(t));
        }
        self.last_kit = sess.last_kit.clone();
        self.last_open_dir = sess.last_open_dir.clone();
        self.last_save_dir = sess.last_save_dir.clone();
        if let Some(kit) = &self.last_kit {
            self.apply_kit(crate::load_kit(kit));
        }
        self.cx = sess.view.cx;
        self.cy = sess.view.cy;
        self.scale = sess.view.scale;
        self.active = sess.view.active.min(self.layers.len());
        self.osm_cache.set_source(crate::BasemapSource::Osm);
        self.show_grid = sess.view.show_grid;
        self.show_labels = sess.view.show_labels;
        self.format = match sess.view.format_str.as_str() {
            "DegMin" => CoordFormat::DegMin,
            "DegMinSec" => CoordFormat::DegMinSec,
            _ => CoordFormat::Decimal,
        };
        
        if let Some(t) = &sess.theme {
            self.current_theme = match t.as_str() {
                "light" => AppTheme::Light,
                _       => AppTheme::Dark,
            };
            let (th, r) = theme_pair_for(&self.ui_theme, self.current_theme);
            self.theme = th;
            self.resolved_theme = r;
        }

        self.undo_stack.clear();
        self.redo_stack.clear();
        self.canvas_cache.clear();
        self.grid_cache.clear();
        self.osm_canvas_cache.clear();
        self.tracks_cache.clear();
        self.status = format!(
            "Сессия загружена ({} треков, {} точек)",
            self.tracks.len(), self.waypoints.len()
        );
        self.maybe_update_declination(true);
    }
    
    fn select_point_range(&mut self, from: (u64, usize, usize), to: (u64, usize, usize)) {
        // Идём по всем выделенным трекам в порядке и находим индексы from и to в плоском списке
        let mut flat: Vec<(u64, usize, usize)> = Vec::new();
        for track_id in &self.selection.tracks {
            if let Some(t) = self.tracks.iter().find(|t| t.id == *track_id) {
                for (seg_idx, seg) in t.segments.iter().enumerate() {
                    for pt_idx in 0..seg.points.len() {
                        flat.push((*track_id, seg_idx, pt_idx));
                    }
                }
            }
        }
        if let (Some(a), Some(b)) = (flat.iter().position(|k| *k == from), flat.iter().position(|k| *k == to)) {
            let (min, max) = if a < b { (a, b) } else { (b, a) };
            for i in min..=max {
                self.selection.points.insert(flat[i]);
            }
        }
    }

    fn select_points_in_rect(&mut self, start: Point, end: Point) {
        let (x_min, x_max) = (start.x.min(end.x), start.x.max(end.x));
        let (y_min, y_max) = (start.y.min(end.y), start.y.max(end.y));
        let sz = self.last_bounds.get();

        // Если есть выделенные треки — ищем только в них.
        // Если нет — ищем во всех видимых треках.
        let track_ids: Vec<u64> = if !self.selection.tracks.is_empty() {
            self.selection.tracks.iter().copied().collect()
        } else {
            self.tracks.iter().filter(|t| t.visible).map(|t| t.id).collect()
        };

        for track_id in track_ids {
            if let Some(t) = self.tracks.iter().find(|t| t.id == track_id) {
                for (seg_idx, seg) in t.segments.iter().enumerate() {
                    for (pt_idx, pt) in seg.points.iter().enumerate() {
                        let (mx, my) = merc_of(pt.lon, pt.lat);
                        let sx = (sz.width as f64 / 2.0 + (mx - self.cx) / self.scale) as f32;
                        let sy = (sz.height as f64 / 2.0 - (my - self.cy) / self.scale) as f32;
                        if sx >= x_min && sx <= x_max && sy >= y_min && sy <= y_max {
                            self.selection.points.insert((track_id, seg_idx, pt_idx));
                        }
                    }
                }
            }
        }
    }

    fn recompute_affected_tracks(&mut self) {
        let mut affected: HashSet<u64> = HashSet::new();
        for (track_id, _, _) in &self.selection.points {
            affected.insert(*track_id);
        }
        for track_id in affected {
            if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                domain::recompute_track_stats(Arc::make_mut(t));
            }
        }
    }

        fn handle_add_waypoint(&mut self, p: Point) {
        let sz = self.last_bounds.get();
        let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
        let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
        let (lon, lat) = lonlat_of(mx, my);

        let new_wp = domain::Waypoint {
            id: self.next_id,
            name: format!("Новая точка {}", self.next_id),
            lon, lat,
            ele: None, time: None, symbol: None, description: None,
            visible: true,
            source: domain::FileSource::New,
            text_color: [0, 0, 0],
            bg_color: [0, 255, 255],
        };
        
        self.next_id += 1;
        self.push_undo(t!("msg_0015"));
        self.waypoints.push(Arc::new(new_wp.clone()));
        
        self.waypoint_backup = Some(new_wp.clone());
        self.modal = Some(Modal::WaypointProps {
            waypoint_id: new_wp.id,
            draft: new_wp,
            coord_input: format!("{:.6} {:.6}", lat, lon),
            is_new: true,
        });
        
        self.active_tool = crate::Tool::SelectPoints;
        self.tracks_cache.clear();
    }

    fn handle_select_points_press(&mut self, p: Point) {
        self.point_press_start = Some(p);
        self.point_is_dragging = false;
        self.point_pressed_key = None;

        let sz = self.last_bounds.get();
        let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
        let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;

        let mut closest_pt = None;
        let mut min_dist = 15.0;

        for track_id in &self.selection.tracks {
            if let Some(t) = self.tracks.iter().find(|t| t.id == *track_id) {
                for (seg_idx, seg) in t.segments.iter().enumerate() {
                    for (pt_idx, pt) in seg.points.iter().enumerate() {
                        let (pt_mx, pt_my) = merc_of(pt.lon, pt.lat);
                        let dx = (pt_mx - mx) / self.scale;
                        let dy = (pt_my - my) / self.scale;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist < min_dist {
                            min_dist = dist;
                            closest_pt = Some((*track_id, seg_idx, pt_idx));
                        }
                    }
                }
            }
        }

        self.point_pressed_key = closest_pt;
        if closest_pt.is_none() {
            self.rubber_band_start = Some(p);
            self.rubber_band_end = Some(p);
        }
    }

    fn handle_select_objects_press(&mut self, p: Point) {
        self.point_press_start = Some(p);
        self.point_is_dragging = false;
        self.rubber_band_start = None;
        self.rubber_band_end = None;
        
        let sz = self.last_bounds.get();
        let hw = sz.width as f64 / 2.0;
        let hh = sz.height as f64 / 2.0;

        // 1. Ищем waypoint
        let mut hit_wp = None;
        for wp in &self.waypoints {
            if !wp.visible { continue; }
            let (mx, my) = merc_of(wp.lon, wp.lat);
            let sx = (hw + (mx - self.cx) / self.scale) as f32;
            let sy = (hh - (my - self.cy) / self.scale) as f32;
            let dist = (p.x - sx).hypot(p.y - sy);
            if dist < 20.0 {
                hit_wp = Some(wp.id);
                break;
            }
        }

        let ctrl = self.modifiers.control() || self.modifiers.command();
        let shift = self.modifiers.shift();

        if let Some(id) = hit_wp {
            if !ctrl && !shift {
                self.selection.tracks.clear();
                self.selection.waypoints.clear();
            }
            if ctrl {
                if !self.selection.waypoints.remove(&id) { self.selection.waypoints.insert(id); }
            } else {
                self.selection.waypoints.insert(id);
            }
            self.point_is_dragging = true;
        } else if let Some(track_id) = self.hit_test_track_at(p, 15.0) {
            if !ctrl && !shift {
                self.selection.tracks.clear();
                self.selection.waypoints.clear();
                self.selection.points.clear();
            }
            if ctrl {
                if !self.selection.tracks.remove(&track_id) { self.selection.tracks.insert(track_id); }
            } else {
                self.selection.tracks.insert(track_id);
            }
            self.point_is_dragging = true;
        } else {
            self.rubber_band_start = Some(p);
            self.rubber_band_end = Some(p);
        }
        self.tracks_cache.clear();
    }

    fn handle_select_points_move(&mut self, p: Point) {
        if let Some(start) = self.point_press_start {
            let dx = p.x - start.x;
            let dy = p.y - start.y;
            let moved = (dx * dx + dy * dy).sqrt();

            if moved > 3.0 && !self.point_is_dragging {
                self.point_is_dragging = true;
                self.move_start_cursor = Some(start);

                if let Some(pt_key) = self.point_pressed_key {
                    let ctrl = self.modifiers.control() || self.modifiers.command();
                    if !self.selection.points.contains(&pt_key) {
                        if !ctrl {
                            self.selection.points.clear();
                        }
                        self.selection.points.insert(pt_key);
                    }
                    self.push_undo(t!("msg_0016"));
                    self.moving_points = true;
                }
            }

            if self.moving_points {
                if let Some(prev) = self.move_start_cursor {
                    let dx_m = (p.x - prev.x) as f64 * self.scale;
                    let dy_m = -(p.y - prev.y) as f64 * self.scale;
                    
                    let points: Vec<_> = self.selection.points.iter().copied().collect();
                    for (track_id, seg_idx, pt_idx) in points {
                        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                            let t_m = Arc::make_mut(t);
                            if let Some(seg) = t_m.segments.get_mut(seg_idx) {
                                if let Some(pt) = seg.points.get_mut(pt_idx) {
                                    let (mx, my) = merc_of(pt.lon, pt.lat);
                                    let (nl, nat) = lonlat_of(mx + dx_m, my + dy_m);
                                    pt.lon = nl;
                                    pt.lat = nat;
                                }
                            }
                        }
                    }
                    self.move_start_cursor = Some(p);
                    self.tracks_cache.clear();
                }
            } else if self.rubber_band_start.is_some() {
                self.rubber_band_end = Some(p);
                self.tracks_cache.clear();
            }
        }
    }

    fn handle_select_points_release(&mut self) {
        let ctrl = self.modifiers.control() || self.modifiers.command();
        let shift = self.modifiers.shift();

        // 1. ПРИОРИТЕТ: Завершение рамки выделения
        if let (Some(s), Some(e)) = (self.rubber_band_start, self.rubber_band_end) {
            if !ctrl && !shift {
                self.selection.points.clear();
            }
            self.select_points_in_rect(s, e);
        }
        // 2. Клик без движения
        else if !self.point_is_dragging {
            if let Some(pt_key) = self.point_pressed_key {
                if ctrl {
                    if !self.selection.points.remove(&pt_key) {
                        self.selection.points.insert(pt_key);
                    }
                } else if shift {
                    if let Some(last) = self.last_point_clicked {
                        self.select_point_range(last, pt_key);
                    } else {
                        self.selection.points.clear();
                        self.selection.points.insert(pt_key);
                    }
                } else {
                    self.selection.points.clear();
                    self.selection.points.insert(pt_key);
                }
                self.last_point_clicked = Some(pt_key);
            } else if !ctrl && !shift {
                self.selection.points.clear();
            }
        }
        // 3. Завершение перемещения точек
        else if self.moving_points {
            self.moving_points = false;
            self.move_start_cursor = None;
            self.recompute_affected_tracks();
        }

        self.point_press_start = None;
        self.point_pressed_key = None;
        self.point_is_dragging = false;
        self.rubber_band_start = None;
        self.rubber_band_end = None;
        self.tracks_cache.clear();
    }

    fn handle_select_objects_move(&mut self, p: Point) {
        if let Some(start) = self.point_press_start {
            let dx = p.x - start.x;
            let dy = p.y - start.y;
            if (dx * dx + dy * dy).sqrt() > 3.0 {
                self.point_is_dragging = true;
                if self.rubber_band_start.is_some() {
                    self.rubber_band_end = Some(p);
                    self.tracks_cache.clear();
                }
            }
        }
    }

    fn handle_select_objects_release(&mut self) {
        if let (Some(s), Some(e)) = (self.rubber_band_start, self.rubber_band_end) {
            let ctrl = self.modifiers.control() || self.modifiers.command();
            let shift = self.modifiers.shift();
            if !ctrl && !shift {
                self.selection.tracks.clear();
                self.selection.waypoints.clear();
            }
            
            let (x_min, x_max) = (s.x.min(e.x), s.x.max(e.x));
            let (y_min, y_max) = (s.y.min(e.y), s.y.max(e.y));
            let sz = self.last_bounds.get();
            let hw = sz.width as f64 / 2.0;
            let hh = sz.height as f64 / 2.0;

            // Выделяем waypoints
            for wp in &self.waypoints {
                if !wp.visible { continue; }
                let (mx, my) = merc_of(wp.lon, wp.lat);
                let sx = (hw + (mx - self.cx) / self.scale) as f32;
                let sy = (hh - (my - self.cy) / self.scale) as f32;
                if sx >= x_min && sx <= x_max && sy >= y_min && sy <= y_max {
                    self.selection.waypoints.insert(wp.id);
                }
            }

            // Выделяем треки, которые ЦЕЛИКОМ внутри рамки
            for t in &self.tracks {
                if !t.visible { continue; }
                let mut fully_inside = true;
                let mut has_pts = false;
                for seg in &t.segments {
                    for pt in &seg.points {
                        has_pts = true;
                        let (mx, my) = merc_of(pt.lon, pt.lat);
                        let sx = (hw + (mx - self.cx) / self.scale) as f32;
                        let sy = (hh - (my - self.cy) / self.scale) as f32;
                        if sx < x_min || sx > x_max || sy < y_min || sy > y_max {
                            fully_inside = false;
                            break;
                        }
                    }
                    if !fully_inside { break; }
                }
                if fully_inside && has_pts {
                    self.selection.tracks.insert(t.id);
                }
            }
        }
        self.rubber_band_start = None;
        self.rubber_band_end = None;
        self.point_press_start = None;
        self.point_is_dragging = false;
        self.tracks_cache.clear();
    }

    fn finish_freehand(&mut self) {
        if !self.freehand_is_drawing { return; }
        self.freehand_is_drawing = false;
        
        let Some(dt) = self.drawing_track.take() else { return };
        if dt.points.len() < 2 {
            self.status = t!("msg_0017").into();
            self.tracks_cache.clear();
            return;
        }

        // Упрощаем через Дуглас-Пекер
        let raw_pts: Vec<domain::TrackPoint> = dt.points.iter()
            .map(|&(lon, lat)| domain::TrackPoint { lon, lat, ele: None, time: None })
            .collect();
        let before = raw_pts.len();
        let simplified = domain::simplify::simplify_points(&raw_pts, self.simplify_tolerance);
        let after = simplified.len();
        
        self.push_undo(t!("msg_0018"));
        
        if let Some(target_id) = dt.target_track_id {
            if let Some(t) = self.tracks.iter_mut().find(|t| t.id == target_id) {
                let t_m = Arc::make_mut(t);
                for seg in &mut t_m.segments {
                    for pt in &mut seg.points {
                        pt.ele = None;
                        pt.time = None;
                    }
                }
                if let Some(last_seg) = t_m.segments.last_mut() {
                    last_seg.points.extend(simplified);
                } else {
                    t_m.segments.push(domain::TrackSegment { points: simplified });
                }
                domain::recompute_track_stats(t_m);
                self.status = format!("Freehand: добавлено {after} точек (упрощено {before} → {after})");
            }
        } else {
            let new_name = format!("Track{}", self.next_id);
            let mut new_track = domain::Track {
                id: self.next_id,
                name: new_name.clone(),
                color: [0x2a, 0x7f, 0xff],
                visible: true,
                segments: vec![domain::TrackSegment { points: simplified }],
                source: domain::FileSource::New,
                props: domain::TrackProps {
                    description: new_name,
                    ..Default::default()
                },
                cached_points: 0,
                cached_distance_m: 0.0,
            };
            domain::recompute_track_stats(&mut new_track);
            let new_id = new_track.id;
            self.next_id += 1;
            self.tracks.push(Arc::new(new_track));

            self.selection.tracks.clear();
            self.selection.tracks.insert(new_id);

            self.status = format!("Freehand: создан трек (упрощено {before} → {after})");
        }
        
        self.tracks_cache.clear();
    }
    
    fn finish_drawing(&mut self) {
        let Some(dt) = self.drawing_track.take() else { return };
        if dt.points.len() < 2 {
            self.status = t!("msg_0017").into();
            return;
        }
        
        self.push_undo(t!("msg_0019"));
        
        let new_pts: Vec<domain::TrackPoint> = dt.points.iter()
            .map(|&(lon, lat)| domain::TrackPoint { lon, lat, ele: None, time: None })
            .collect();
        
        if let Some(target_id) = dt.target_track_id {
            if let Some(t) = self.tracks.iter_mut().find(|t| t.id == target_id) {
                let t_m = Arc::make_mut(t);
                // Стираем лишнюю инфу у всех существующих точек
                for seg in &mut t_m.segments {
                    for pt in &mut seg.points {
                        pt.ele = None;
                        pt.time = None;
                    }
                }
                // Добавляем новые точки к последнему сегменту
                if let Some(last_seg) = t_m.segments.last_mut() {
                    last_seg.points.extend(new_pts);
                } else {
                    t_m.segments.push(domain::TrackSegment { points: new_pts });
                }
                domain::recompute_track_stats(t_m);
                self.status = format!("Добавлено {} точек к треку", dt.points.len());
            }
        } else {
            let new_name = format!("Track{}", self.next_id);
            let mut new_track = domain::Track {
                id: self.next_id,
                name: new_name.clone(),
                color: [0x2a, 0x7f, 0xff],
                visible: true,
                segments: vec![domain::TrackSegment { points: new_pts }],
                source: domain::FileSource::New,
                props: domain::TrackProps {
                    description: new_name,
                    ..Default::default()
                },
                cached_points: 0,
                cached_distance_m: 0.0,
            };
            domain::recompute_track_stats(&mut new_track);
            let new_id = new_track.id;
            self.next_id += 1;
            self.tracks.push(Arc::new(new_track));

            // Автовыделение нового трека (для «без Shift» сценария).
            self.selection.tracks.clear();
            self.selection.tracks.insert(new_id);

            self.status = format!("Создан новый трек ({} точек)", dt.points.len());
        }
        
        self.tracks_cache.clear();
    }

    // =========================================================================
    // 1. Логика View (Панорамирование, зум, ресайз окна)
    // =========================================================================
    fn update_view(&mut self, m: Message) {
        match m {
             
            Message::MapPressed(p) => {
                self.cursor = Some(p);
                self.search_focused = false;
                
                if self.active_tool == crate::Tool::Pan || self.space_pan_active {
                    self.dragging = true;
                } else if self.active_tool == crate::Tool::AddWaypoint {
                    self.handle_add_waypoint(p);
                } else if self.active_tool == crate::Tool::SelectPoints {
                    self.handle_select_points_press(p);
                } else if self.active_tool == crate::Tool::SelectObjects {
                    self.handle_select_objects_press(p);
                
                } else if self.active_tool == crate::Tool::DrawTrack {
                    let sz = self.last_bounds.get();
                    let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
                    let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
                    let (lon, lat) = lonlat_of(mx, my);

                    if self.drawing_track.is_none() {
                        let shift = self.modifiers.shift();
                        let append = shift && self.selection.tracks.len() == 1;

                        let target = if append {
                            self.selection.tracks.iter().next().copied()
                        } else {
                            None
                        };

                        if !append {
                            self.selection.tracks.clear();
                            self.selection.waypoints.clear();
                            self.selection.points.clear();
                        }

                        self.drawing_track = Some(DrawingTrack { points: Vec::new(), target_track_id: target });
                    }

                    if let Some(dt) = &mut self.drawing_track {
                        dt.points.push((lon, lat));
                        self.tracks_cache.clear();
                    }
                } else if self.active_tool == crate::Tool::Freehand {
                    let sz = self.last_bounds.get();
                    let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
                    let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
                    let (lon, lat) = lonlat_of(mx, my);

                    if self.drawing_track.is_some() {
                        self.finish_freehand();
                    }

                    // Решение о target откладываем до первого Moved (см. ниже).
                    // Это защищает от случайного клика без Shift, который сбросил бы
                    // выделение до того, как началось реальное рисование.
                    self.drawing_track = Some(DrawingTrack { points: vec![(lon, lat)], target_track_id: None });
                    self.freehand_is_drawing = true;
                    self.tracks_cache.clear();
                } else if self.active_tool == crate::Tool::Ruler {
                    let sz = self.last_bounds.get();
                    let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
                    let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
                    let (lon, lat) = lonlat_of(mx, my);
                    
                    self.ruler_points.push((lon, lat));
                    self.tracks_cache.clear();
                }
            }
            
            Message::Moved(p) => {
                if self.dragging {
                    if let Some(prev) = self.cursor {
                        self.cx -= (p.x - prev.x) as f64 * self.scale;
                        self.cy += (p.y - prev.y) as f64 * self.scale;
                        self.canvas_cache.clear();
                        self.grid_cache.clear();
                        self.osm_canvas_cache.clear();
                        self.tracks_cache.clear();
                    }
                    self.cursor = Some(p);
                    self.maybe_update_declination(false); 
                } else if self.active_tool == crate::Tool::SelectPoints {
                    self.cursor = Some(p);
                    self.handle_select_points_move(p);
                } else if self.active_tool == crate::Tool::SelectObjects {
                    self.cursor = Some(p);
                    self.handle_select_objects_move(p);
                } else if self.active_tool == crate::Tool::Freehand {
                    if self.freehand_is_drawing {
                        if let Some(dt) = &mut self.drawing_track {
                            let sz = self.last_bounds.get();
                            let mx = self.cx + (p.x as f64 - sz.width as f64 / 2.0) * self.scale;
                            let my = self.cy - (p.y as f64 - sz.height as f64 / 2.0) * self.scale;
                            let (lon, lat) = lonlat_of(mx, my);
                            
                            // Добавляем точку только если сдвиг >= 3 пикселей
                            let should_add = if let Some(&(last_lon, last_lat)) = dt.points.last() {
                                let (lmx, lmy) = merc_of(last_lon, last_lat);
                                let dx_px = (lmx - mx) / self.scale;
                                let dy_px = (lmy - my) / self.scale;
                                (dx_px * dx_px + dy_px * dy_px).sqrt() >= 3.0
                            } else {
                                true
                            };
                            
                            if should_add {
                                // Первый Moved после press: решаем target с учётом Shift.
                                if dt.points.len() == 1 {
                                    let shift = self.modifiers.shift();
                                    let append = shift && self.selection.tracks.len() == 1;
                                    if append {
                                        dt.target_track_id = self.selection.tracks.iter().next().copied();
                                    } else {
                                        dt.target_track_id = None;
                                        if !shift {
                                            self.selection.tracks.clear();
                                            self.selection.waypoints.clear();
                                            self.selection.points.clear();
                                        }
                                    }
                                }
                                dt.points.push((lon, lat));
                                self.tracks_cache.clear();
                            }
                        }
                    }
                    self.cursor = Some(p);
                    self.refresh_status();
                } else {
                    self.cursor = Some(p);
                    self.refresh_status();
                }
            }
            Message::MapReleased(p) => {
                self.cursor = Some(p);
                
                if self.dragging {
                    self.dragging = false;
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.maybe_update_declination(true);
                    self.refresh_status();
                } else if self.active_tool == crate::Tool::SelectPoints {
                    self.handle_select_points_release();
                } else if self.active_tool == crate::Tool::SelectObjects {
                    self.handle_select_objects_release();
                } else if self.active_tool == crate::Tool::Freehand {
                    self.finish_freehand();
                }
            }
            
            Message::Scrolled(d) => {
                let steps = match d {
                    ScrollDelta::Lines { y, .. } => y as f64,
                    ScrollDelta::Pixels { y, .. } => y as f64 / 40.0,
                };
                let sz = self.last_bounds.get();
                let p = self.cursor.unwrap_or(Point::new(sz.width / 2.0, sz.height / 2.0));
                let (ox, oy) = (
                    p.x as f64 - sz.width as f64 / 2.0,
                    p.y as f64 - sz.height as f64 / 2.0,
                );
                let (mx, my) = (self.cx + ox * self.scale, self.cy - oy * self.scale);
                self.scale = (self.scale * 0.8f64.powf(steps)).clamp(0.05, 2000.0);
                self.cx = mx - ox * self.scale;
                self.cy = my + oy * self.scale;
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                self.maybe_update_declination(false); 
                self.refresh_status();
            }
            Message::Fit => {
                // Приоритет: сетка → локальная карта. OSM — только если нет ни сетки, ни локальной.
                let did_fit = if self.fit_to_grid() {
                    true
                } else if !self.osm_active() {
                    let idx = self.active;
                    self.fit_to_layer(idx);
                    true
                } else {
                    false
                };

                if did_fit {
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear(); 
                    self.maybe_update_declination(true); 
                    self.refresh_status();
                }
            }
            Message::WindowResized(size) => self.window_size = size,
            Message::WindowMoved(pos) => self.window_pos = pos,
            Message::DividerPressed => self.resizing_panel = true,
            Message::CursorMoved(p) => {
                if self.modal_dragging {
                    // Если это первое движение мыши после клика — запоминаем точку старта
                    if self.modal_drag_start_cursor.is_none() {
                        self.modal_drag_start_cursor = Some(p);
                    }
                    
                    if let Some(start) = self.modal_drag_start_cursor {
                        let dx = p.x - start.x;
                        let dy = p.y - start.y;
                        self.modal_drag_offset = Point::new(
                            self.modal_drag_start_offset.x + dx,
                            self.modal_drag_start_offset.y + dy,
                        );
                    }
                }
                if self.resizing_panel {
                    let win_w = self.window_size.width;
                    let new_w = (win_w - p.x).clamp(200.0, win_w * 0.6);
                    self.right_panel_width = new_w;
                }
            }
            Message::DividerReleased => {
                self.resizing_panel = false;
                self.modal_dragging = false;
            }
            _ => {}
        }
    }

    // =========================================================================
    // 2. UI (Сетка, формат)
    // =========================================================================
    /// Кнопка на верхней панели запускает сервер и открывает окно статуса.
    fn start_foxydrop(&mut self) -> Task<Message> {
        if self.drop_phase != crate::foxydrop::Phase::Stopped { return Task::none(); }
        self.drop_error = None;
        self.drop_ips = crate::foxydrop::local_ipv4_addresses();
        if self.drop_selected_ip.map_or(true, |ip| !self.drop_ips.contains(&ip)) {
            self.drop_selected_ip = self.drop_ips.first().copied();
        }
        let Some(ip) = self.drop_selected_ip else {
            self.drop_error = Some("Не найден IPv4-адрес локальной сети".into());
            return Task::none();
        };
        self.drop_phase = crate::foxydrop::Phase::Starting;
        let service = Arc::clone(&self.drop_service);
        let port = crate::config::load().drop_port;
        Task::perform(async move { service.start(ip, port).await }, Message::FoxyDropStarted)
    }

    fn stop_foxydrop(&mut self, restart: bool) -> Task<Message> {
        if self.drop_phase != crate::foxydrop::Phase::Running { return Task::none(); }
        self.drop_restart_after_stop = restart;
        self.drop_phase = crate::foxydrop::Phase::Stopping;
        match self.drop_service.stop() {
            Some(join) => Task::perform(
                async move { crate::foxydrop::DropService::finish_stop(join).await },
                |_| Message::FoxyDropStopped,
            ),
            None => Task::done(Message::FoxyDropStopped),
        }
    }

    fn update_ui(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::KeyPressed(key, modifiers) => {
                if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) = key {
                    if self.context_menu.is_some() {
                        self.context_menu = None;
                        return Task::none();
                    }
                }
                // Модальное окно — закрыть по Escape (как кнопка «Отмена»)
                if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) = key {
                    if self.modal.is_some() {
                        let close_msg = match &self.modal {
                            Some(Modal::TrackProps { .. })            => Message::CloseTrackProps,
                            Some(Modal::WaypointProps { .. })         => Message::CloseWaypointProps,
                            Some(Modal::TrackList(_))                 => Message::CloseTrackList,
                            Some(Modal::ConfirmClearAll)              => Message::CancelClearAll,
                            Some(Modal::ConfirmLoadSession)           => Message::CancelClearAll,
                            Some(Modal::WaypointMeasure { .. })       => Message::CloseWaypointMeasure,
                            Some(Modal::WaypointProjection { .. })    => Message::CloseWaypointProjection,
                            Some(Modal::About)                        => Message::CloseAbout,
                            Some(Modal::Settings { .. })              => Message::CloseSettings,
                            Some(Modal::FoxyDrop)                    => Message::CloseFoxyDrop,
                            None                                      => return Task::none(),
                        };
                        return self.update(close_msg);
                    }
                }
                // Если открыто модальное окно — игнорируем всё, кроме Ctrl+Z/Ctrl+Y/Ctrl+A и стрелок
                let is_modal_open = self.modal.is_some();
                

                // Управление рисованием
                if (self.active_tool == crate::Tool::DrawTrack || self.active_tool == crate::Tool::Freehand) 
                    && self.drawing_track.is_some() 
                {
                    match &key {
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
                            self.freehand_is_drawing = false;
                            self.drawing_track = None;
                            self.tracks_cache.clear();
                            self.status = t!("msg_0020").into();
                            return Task::none();
                        }
                        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) => {
                            if self.freehand_is_drawing {
                                self.finish_freehand();
                            } else {
                                self.finish_drawing();
                            }
                            return Task::none();
                        }
                        _ => {}
                    }
                }
                if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) = key {
                    if self.search_focused {
                        self.search_focused = false;
                        return Task::none();
                    }
                }
                if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) = key {
                    self.compact_view = !self.compact_view;
                }
                if !is_modal_open && !self.search_focused {
                    // 1. Инструменты (без модификаторов, с учетом обеих раскладок)
                    if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) = key {
                        if self.active_tool == crate::Tool::Ruler {
                            self.ruler_points.clear();
                            self.tracks_cache.clear();
                            self.status = t!("msg_0021").into();
                            return Task::none();
                        }
                    }
                    if !modifiers.command() && !modifiers.control() && !modifiers.alt() {
                        let tool = match &key {
                            iced::keyboard::Key::Character(c) if c == "q" || c == t!("msg_0022") => Some(crate::Tool::Pan),
                            iced::keyboard::Key::Character(c) if c == "s" || c == t!("msg_0023") => Some(crate::Tool::SelectObjects),
                            iced::keyboard::Key::Character(c) if c == "d" || c == t!("msg_0024") => Some(crate::Tool::SelectPoints),
                            iced::keyboard::Key::Character(c) if c == "t" || c == t!("msg_0025") => Some(crate::Tool::DrawTrack),
                            iced::keyboard::Key::Character(c) if c == "f" || c == t!("msg_0026") => Some(crate::Tool::Freehand),
                            iced::keyboard::Key::Character(c) if c == "w" || c == t!("msg_0027") => Some(crate::Tool::AddWaypoint),
                            iced::keyboard::Key::Character(c) if c == "r" || c == t!("msg_0028") => Some(crate::Tool::Ruler),
                            _ => None,
                        };
                        if let Some(t) = tool {
                            self.active_tool = t;
                            self.dragging = false;
                            self.canvas_cache.clear();
                            self.grid_cache.clear();
                            self.osm_canvas_cache.clear();
                            self.tracks_cache.clear();
                            return Task::none();
                        }
                    }

                    // 2. Space (только если не печатаем в поле)
                    if is_space(&key) {
                        self.space_pan_active = true;
                        return Task::none();
                    }

                    // 3. Удаление точек (только если активен SelectPoints)
                    if self.active_tool == crate::Tool::SelectPoints && !self.selection.points.is_empty() {
                        if let iced::keyboard::Key::Named(iced::keyboard::key::Named::Delete) 
                            | iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace) = key {
                            self.update_tracks(Message::DeleteSelectedPoints);
                            return Task::none();
                        }
                    }
                }

                // 4. Стрелки для таблицы точек (работают всегда, если открыта TrackList)
                match &key {
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) => {
                        return self.update_track_list(Message::TrackListUp);
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) => {
                        return self.update_track_list(Message::TrackListDown);
                    }
                    _ => {}
                }

                // 5. Горячие клавиши с Ctrl/Cmd (работают всегда)
                // 5. Ctrl+Z, Ctrl+Y, Ctrl+A (с учётом русской раскладки)
                if modifiers.command() {
                    match &key {
                        // Z | Я (или заглавные при Shift)
                        iced::keyboard::Key::Character(c) 
                            if c == "z" || c == "Z" || c == t!("msg_0029") || c == t!("msg_0030") => 
                        {
                            if modifiers.shift() { 
                                self.perform_redo(); 
                            } else { 
                                self.perform_undo(); 
                            }
                        }
                        // Y | Н (или заглавные)
                        iced::keyboard::Key::Character(c) 
                            if c == "y" || c == "Y" || c == t!("msg_0031") || c == t!("msg_0032") => 
                        {
                            self.perform_redo();
                        }
                        // A | Ф (или заглавные)
                        iced::keyboard::Key::Character(c) 
                            if c == "a" || c == "A" || c == t!("msg_0033") || c == t!("msg_0034") => 
                        {
                            if let Some(Modal::TrackList(state)) = &mut self.modal {
                                state.selected_points.clear();
                                for r in &state.rows {
                                    state.selected_points.insert((r.sect, r.pt_idx));
                                }
                                self.sync_tracklist_selection();
                                self.tracks_cache.clear();
                            }
                        }
                        // G | П — открыть Геоданные (как кнопка «Геоданные» в тулбаре)
                        iced::keyboard::Key::Character(c) 
                            if c == "g" || c == "G" || c == t!("msg_0035") || c == t!("msg_0036") => 
                        {
                            return self.update_system(Message::PickTrack);
                        }
                        iced::keyboard::Key::Character(c) 
                            if c == "d" || c == "D" || c == t!("msg_0024") || c == t!("msg_0037") => 
                        {
                            self.selection.tracks.clear();
                            self.selection.waypoints.clear();
                            self.selection.points.clear();

                            // Сбрасываем якоря Shift-диапазона, чтобы следующий Shift-клик
                            // не выделял «от старой позиции».
                            self.last_track_clicked = None;
                            self.last_waypoint_clicked = None;
                            self.last_point_clicked = None;

                            // Если открыта таблица точек — снимаем выделение и там.
                            if let Some(Modal::TrackList(state)) = &mut self.modal {
                                state.selected_points.clear();
                                state.last_clicked = None;
                            }

                            self.tracks_cache.clear();
                            self.status = t!("msg_0038").into();
                        }
                        // S | Ы — сохранить сессию
                        iced::keyboard::Key::Character(c) 
                            if c == "s" || c == "S" || c == t!("msg_0023") || c == t!("msg_0039") => 
                        {
                            return self.update_ui(Message::SaveSessionAs);
                        }
                        // O | Щ — открыть сессию
                        iced::keyboard::Key::Character(c) 
                            if c == "o" || c == "O" || c == t!("msg_0040") || c == t!("msg_0041") => 
                        {
                            return self.update_ui(Message::LoadSessionPrompt);
                        }
                        _ => {}
                    }
                }
            }
            Message::KeyReleased(key) => {
                if is_space(&key) {
                    self.space_pan_active = false;
                }
            }
            Message::ToggleGrid => {
                self.show_grid = !self.show_grid;
                self.grid_cache.clear();
            }
            Message::ToggleLabels => {
                self.show_labels = !self.show_labels;
                self.grid_cache.clear();
            }
            Message::CycleFormat => {
                self.format = match self.format {
                    CoordFormat::Decimal => CoordFormat::DegMin,
                    CoordFormat::DegMin => CoordFormat::DegMinSec,
                    CoordFormat::DegMinSec => CoordFormat::Decimal,
                };
                self.refresh_status();
            }
            Message::SearchTracksChanged(q) => {
                self.search_tracks = q;
                self.search_focused = true;      // ← добавить
                self.tracks_cache.clear();
            }
            Message::SearchWaypointsChanged(q) => {
                self.search_waypoints = q;
                self.search_focused = true;      // ← добавить
                self.tracks_cache.clear();
            }
            Message::ModalDragStart => {
                if self.modal.is_some() {
                    self.modal_dragging = true;
                    self.modal_drag_start_cursor = None;
                    self.modal_drag_start_offset = self.modal_drag_offset;
                }
            }
            Message::SelectTool(tool) => {
                if tool != crate::Tool::Ruler || self.active_tool == crate::Tool::Ruler {
                    // Очищаем, если уходим с линейки ИЛИ кликнули по ней же (чтобы начать заново)
                    if !self.ruler_points.is_empty() {
                        self.ruler_points.clear();
                        self.tracks_cache.clear();
                    }
                }
                // Если было активное рисование — завершаем
                if self.freehand_is_drawing {
                    self.finish_freehand();
                } else if self.drawing_track.is_some() {
                    // Это был DrawTrack
                    let needs_finish = self.drawing_track.as_ref().map_or(false, |dt| dt.points.len() >= 2);
                    if needs_finish {
                        self.finish_drawing();
                    } else {
                        self.drawing_track = None;
                    }
                }
                
                self.active_tool = tool;
                self.search_focused = false;
                self.dragging = false;
                self.modal = None;
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
            }
            Message::SaveSessionAs => {
                let dialog = rfd::AsyncFileDialog::new()
                    .set_file_name("session.foxy")
                    .add_filter("Foxy Session", &["foxy"]);
                return Task::perform(
                    async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) },
                    Message::SaveSessionAsPicked,
                );
            }
            Message::SaveSessionAsPicked(Some(path)) => {
                let sess = crate::build_session(self);
                session::write_session_to(&sess, &path);
                self.status = format!("{} {}", t!("msg_0227"), path.display());
            }
            Message::SaveSessionAsPicked(None) => {}

            Message::LoadSessionPrompt => {
                // Если сессия пустая — сразу открываем диалог
                if self.tracks.is_empty() && self.waypoints.is_empty() && self.layers.is_empty() {
                    return self.update_ui(Message::LoadSessionNoPrompt);
                }
                self.modal = Some(Modal::ConfirmLoadSession);
            }
            Message::LoadSessionNoPrompt => {
                self.modal = None;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Foxy Session", &["foxy", "json"])
                            .pick_file().await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::LoadSessionPicked,
                );
            }
            Message::SaveAndLoadSession => {
                // Сначала сохраняем текущее состояние, потом загружаем бэкап
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_file_name("session.foxy")
                            .add_filter("Foxy Session", &["foxy"])
                            .save_file().await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::SaveAndLoadSessionSaved,
                );
            }
            Message::SaveAndLoadSessionSaved(Some(save_path)) => {
                let sess = crate::build_session(self);
                session::write_session_to(&sess, &save_path);
                self.modal = None;
                // После сохранения сразу открываем диалог загрузки
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Foxy Session", &["foxy", "json"])
                            .pick_file().await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::LoadSessionPicked,
                );
            }
            Message::SaveAndLoadSessionSaved(None) => { self.modal = None; }

            Message::LoadSessionPicked(Some(path)) => {
                if let Some(sess) = session::load_session_from(&path) {
                    self.apply_session(sess);
                } else {
                    self.status = format!("{} {}", t!("msg_0228"), path.display());
                }
            }
            Message::LoadSessionPicked(None) => {}
            Message::ConfirmClearAll => {
                self.modal = Some(Modal::ConfirmClearAll);
            }
            Message::CancelClearAll => {
                self.modal = None;
            }
            Message::ExecuteClearAll => {
                self.modal = None;
                
                // --- Данные ---
                self.layers.clear();
                self.active = 0;
                self.grid = None;
                self.lines.clear();
                self.tracks.clear();
                self.waypoints.clear();
                self.selection = Selection::default();
                self.next_id = 0;
                self.last_kit = None;
                self.undo_stack.clear();
                self.redo_stack.clear();
                
                // --- Временные состояния инструментов ---
                self.dragging = false;
                self.resizing_panel = false;
                self.moving_points = false;
                self.point_is_dragging = false;
                self.point_press_start = None;
                self.point_pressed_key = None;
                self.last_point_clicked = None;
                self.last_track_click = None;
                self.last_waypoint_click = None;
                self.last_track_clicked = None;
                self.last_waypoint_clicked = None;
                self.rubber_band_start = None;
                self.rubber_band_end = None;
                self.drawing_track = None;
                self.freehand_is_drawing = false;
                self.ruler_points.clear();
                self.waypoint_crosshair = None;
                self.track_backup = None;
                self.waypoint_backup = None;
                self.show_color_picker = None;
                self.show_track_save_menu = false;
                self.show_wp_save_menu = false;
                self.space_pan_active = false;
                self.active_tool = crate::Tool::Pan;
                
                let cfg = crate::config::load(); 
                let (lon, lat) = (cfg.default_lon, cfg.default_lat);
                let (cx, cy) = crate::util::geo::merc_of(lon, lat);
                self.cx = cx;
                self.cy = cy;
                self.scale = DEFAULT_SCALE;
                
                // --- Пересчёт магнитного склонения для нового центра ---
                let (y, ord) = crate::util::coords::today();
                self.declination = declination::declination_deg(lat, lon, y, ord).unwrap_or(0.0);
                
                // --- Кэши ---
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                self.osm_cache.set_source(crate::BasemapSource::Osm);
                self.tile_handles.borrow_mut().clear();
                self.tile_fail_count.clear();
                self.osm_z = 0;
                self.tile_handles.borrow_mut().clear();
                
                // --- Файлы сессии на диске ---
                session::clear_session_files();
                
                self.status = t!("msg_0042").into();
                
                // --- Запрашиваем тайлы OSM для нового центра ---
                return self.request_osm_tiles();
            }
            Message::GlobalCursorMoved(p) => {
                self.global_cursor = p;
            }
            Message::ShowContextMenu(pos, items) => {
                self.context_menu = Some(ContextMenuState {
                    x: pos.x,
                    y: pos.y,
                    items,
                });
            }
            Message::ContextMenuClose => {
                self.context_menu = None;
            }
            Message::ContextMenuSelect(idx) => {
                if let Some(state) = self.context_menu.take() {
                    if let Some(item) = state.items.into_iter().nth(idx) {
                        if let ContextMenuItem::Action(_, action) = item {
                            return self.update(action);
                        }
                    }
                }
            }
            Message::CopyCoordinates(lat, lon) => {
                let text = format!("{:.6} {:.6}", lat, lon);
                self.status = format!("{} {}", t!("msg_0229"), text);
                return iced::clipboard::write(text);
            }
            Message::ToggleTheme => {
                self.current_theme = match self.current_theme {
                    AppTheme::Light => AppTheme::Dark,
                    AppTheme::Dark  => AppTheme::Light,
                };
                let (t, r) = theme_pair_for(&self.ui_theme, self.current_theme);
                self.theme = t;
                self.resolved_theme = r;

                // Кэши надо сбросить — цвета линий/подписей на канвасах завязаны на тему.
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
            }
            Message::OpenAbout => {
                self.modal_drag_offset = Point::ORIGIN;
                self.modal = Some(Modal::About);
            }
            Message::CloseAbout => {
                self.modal_drag_offset = Point::ORIGIN;
                self.modal = None;
            }
            Message::OpenUrl(url) => {
                let _ = open::that(&url);
                self.status = format!("{} {}", t!("msg_0230"), url);
            }
            Message::OpenSettings => {
                self.modal_drag_offset = Point::ORIGIN;
                self.show_color_picker = None;
                let cfg = crate::config::load();
                let coord_input = format!("{:.6} {:.6}", cfg.default_lat, cfg.default_lon);
                self.modal = Some(Modal::Settings {
                    draft: SettingsDraft {
                        coord_input: coord_input.clone(),
                        parsed_lat: Some(cfg.default_lat),
                        parsed_lon: Some(cfg.default_lon),
                        lang: cfg.lang.clone(),
                        simplify_tolerance: cfg.simplify_tolerance,
                        grid_color: cfg.grid_color,
                        drop_folder: cfg.drop_folder.clone(),
                        ui_theme: self.ui_theme.clone(),
                    },
                });
            }
            Message::CloseSettings => {
                self.modal_drag_offset = Point::ORIGIN;
                self.show_color_picker = None;
                self.modal = None;
            }
            Message::ApplySettings => {
                if let Some(Modal::Settings { draft }) = &self.modal {
                    match (draft.parsed_lat, draft.parsed_lon) {
                        (Some(lat), Some(lon)) => {
                            let new_cfg = crate::config::Config {
                                default_lon: lon,
                                default_lat: lat,
                                lang: draft.lang.clone(),
                                simplify_tolerance: draft.simplify_tolerance,
                                grid_color: draft.grid_color,
                                drop_folder: draft.drop_folder.clone(),
                                drop_port: crate::config::load().drop_port,
                                ui_theme: draft.ui_theme.to_string(),
                            };
                            match crate::config::save(&new_cfg) {
                                Ok(_) => {
                                    self.simplify_tolerance = new_cfg.simplify_tolerance;
                                    self.ui_theme = crate::ui_theme_from_name(&new_cfg.ui_theme);
                                    let (theme, resolved) = theme_pair_for(&self.ui_theme, self.current_theme);
                                    self.theme = theme;
                                    self.resolved_theme = resolved;
                                    self.canvas_cache.clear();
                                    self.osm_canvas_cache.clear();
                                    self.tracks_cache.clear();
                                    self.grid_color = new_cfg.grid_color;
                                    self.drop_service.set_directory(PathBuf::from(&new_cfg.drop_folder));
                                    self.grid_cache.clear();
                                    self.show_color_picker = None;
                                    crate::i18n::set_lang(Box::leak(new_cfg.lang.clone().into_boxed_str()));
                                    self.status = "Настройки сохранены".into();
                                    self.modal = None;
                                }
                                Err(e) => {
                                    self.status = format!("Ошибка сохранения: {e}");
                                }
                            }
                        }
                        _ => {
                            self.status = "Некорректные координаты".into();
                        }
                    }
                }
            }
            Message::SettingsCoordChanged(s) => {
                if let Some(Modal::Settings { draft }) = &mut self.modal {
                    draft.coord_input = s.clone();
                    // Пытаемся распарсить — если успешно, обновляем parsed_lat/parsed_lon
                    if let Some((lat, lon)) = parse_coords(&s, self.grid.as_ref(), self.declination) {
                        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
                            draft.parsed_lat = Some(lat);
                            draft.parsed_lon = Some(lon);
                        }
                    }
                }
            }
            Message::SettingsLangChanged(label) => {
                // label приходит как "Русский" / "English" — конвертируем в код
                let code = match label.as_str() {
                    "English" => "en".to_string(),
                    _ => "ru".to_string(),
                };
                if let Some(Modal::Settings { draft }) = &mut self.modal {
                    draft.lang = code;
                }
            }
            Message::SettingsToleranceChanged(v) => {
                if let Some(Modal::Settings { draft }) = &mut self.modal {
                    // Округляем до десятых
                    draft.simplify_tolerance = (v * 10.0).round() / 10.0;
                }
            }
            Message::SettingsChooseDropFolder => {
                let mut dlg = rfd::AsyncFileDialog::new();
                if let Some(Modal::Settings { draft }) = &self.modal {
                    if std::path::Path::new(&draft.drop_folder).is_dir() {
                        dlg = dlg.set_directory(&draft.drop_folder);
                    }
                }
                return Task::perform(
                    async move { dlg.pick_folder().await.map(|h| h.path().to_path_buf()) },
                    Message::SettingsDropFolderPicked,
                );
            }
            Message::SettingsDropFolderPicked(Some(dir)) => {
                if let Some(Modal::Settings { draft }) = &mut self.modal {
                    draft.drop_folder = dir.to_string_lossy().to_string();
                }
            }
            Message::SettingsDropFolderPicked(None) => {}
            Message::SettingsThemeChanged(theme) => {
                if let Some(Modal::Settings { draft }) = &mut self.modal {
                    draft.ui_theme = theme;
                }
            }
            Message::SettingsGridColorChanged(color) => {
                if let Some(Modal::Settings { draft }) = &mut self.modal {
                    draft.grid_color = color;
                    self.show_color_picker = None;
                }
            }
            Message::OpenFoxyDrop => {
                // Кнопка панели только показывает состояние; запуск — исключительно Start.
                self.modal_drag_offset = Point::ORIGIN;
                self.modal = Some(Modal::FoxyDrop);
            }
            Message::CloseFoxyDrop => { self.modal = None; }
            Message::FoxyDropStart => { return self.start_foxydrop(); }
            Message::FoxyDropStop => { return self.stop_foxydrop(false); }
            Message::FoxyDropIpSelected(ip) => {
                if !self.drop_ips.contains(&ip) { return Task::none(); }
                if self.drop_selected_ip == Some(ip) { return Task::none(); }
                self.drop_selected_ip = Some(ip);
                if self.drop_phase == crate::foxydrop::Phase::Running {
                    return self.stop_foxydrop(true);
                }
            }
            Message::FoxyDropStarted(result) => {
                match result {
                    Ok(addr) => {
                        self.drop_address = Some(addr);
                        self.drop_phase = crate::foxydrop::Phase::Running;
                        self.drop_error = None;
                        self.status = format!("FoxyDrop: {}", crate::foxydrop::url(addr));
                    }
                    Err(e) => {
                        self.drop_phase = crate::foxydrop::Phase::Stopped;
                        self.drop_address = None;
                        self.drop_error = Some(e.clone());
                        self.status = format!("FoxyDrop: {e}");
                    }
                }
            }
            Message::FoxyDropStopped => {
                self.drop_phase = crate::foxydrop::Phase::Stopped;
                self.drop_address = None;
                self.status = "FoxyDrop остановлен".into();
                if self.drop_restart_after_stop {
                    self.drop_restart_after_stop = false;
                    return self.start_foxydrop();
                }
            }
            Message::FoxyDropCopyUrl => {
                if let Some(addr) = self.drop_address {
                    return iced::clipboard::write(crate::foxydrop::url(addr));
                }
            }
            Message::FoxyDropHeartbeat => {
                if self.drop_phase == crate::foxydrop::Phase::Running
                    && !self.drop_service.is_running() {
                    let _ = self.drop_service.stop();
                    self.drop_phase = crate::foxydrop::Phase::Stopped;
                    self.drop_address = None;
                    self.drop_error = Some("Сервер неожиданно остановился".into());
                }
            }
            _ => {}
        }
        Task::none() // <-- в самом конце
    }

    // =========================================================================
    // 3. Треки (Логика списка)
    // =========================================================================
    fn update_tracks(&mut self, m: Message) {
        match m {
            Message::Layer(i) => {
                if i <= self.layers.len() + 2 {
                    self.tile_fail_count.clear();
                    self.active = i;
                    if let Some(src) = self.online_basemap() {
                        self.osm_cache.set_source(src);
                    }
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.refresh_status();
                }
            }
            Message::SelectTrack(id) => {
                self.search_focused = false;
                let now = Instant::now();
                let is_double_click = self.last_track_click.map_or(false, |(last_id, last_time)| {
                    last_id == id && now.duration_since(last_time).as_millis() < 300
                });

                if is_double_click {
                    self.modal_drag_offset = Point::ORIGIN;
                    self.last_track_click = None;
                    if let Some(t) = self.tracks.iter().find(|t| t.id == id) {
                        self.track_backup = Some(t.as_ref().clone());
                        self.modal = Some(Modal::TrackProps { track_id: id, draft: t.props.clone() });
                    }
                } else {
                    self.last_track_click = Some((id, now));
                    let ctrl = self.modifiers.control() || self.modifiers.command();
                    let shift = self.modifiers.shift();

                    if shift {
                        if let Some(last_id) = self.last_track_clicked {
                            if let (Some(start), Some(end)) = (
                                self.tracks.iter().position(|t| t.id == last_id),
                                self.tracks.iter().position(|t| t.id == id),
                            ) {
                                let (min, max) = if start < end { (start, end) } else { (end, start) };
                                for i in min..=max {
                                    self.selection.tracks.insert(self.tracks[i].id);
                                }
                            }
                        } else {
                            self.selection.tracks.insert(id);
                        }
                    } else if ctrl {
                        if !self.selection.tracks.remove(&id) {
                            self.selection.tracks.insert(id);
                        }
                    } else {
                        self.selection.tracks.clear();
                        self.selection.waypoints.clear();
                        self.selection.tracks.insert(id);
                    }
                    self.last_track_clicked = Some(id);
                    self.tracks_cache.clear(); 
                }
            }
            Message::ToggleTrackVisible(id, v) => {
                if self.modifiers.alt() {
                    // Solo-режим: показываем только кликнутый, остальные скрываем
                    self.push_undo(t!("msg_0043"));
                    for t in &mut self.tracks {
                        let should_be_visible = t.id == id;
                        if t.visible != should_be_visible {
                            Arc::make_mut(t).visible = should_be_visible;
                        }
                    }
                } else {
                    // Обычное переключение
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                        if t.visible != v {
                            Arc::make_mut(t).visible = v;
                        }
                    }
                }
                self.tracks_cache.clear();
            }
            Message::ToggleAllTracksVisible => {
                let all_visible = self.tracks.iter().all(|t| t.visible);
                for t in &mut self.tracks {
                    Arc::make_mut(t).visible = !all_visible; 
                }
                self.tracks_cache.clear();
            }
             Message::DeleteSelectedTracks => {
                self.push_undo(t!("msg_0044")); 
                let ids = self.selection.tracks.clone();
                self.tracks.retain(|t| !ids.contains(&t.id));
                self.selection.tracks.clear();
                self.selection.waypoints.clear();
                self.tracks_cache.clear();
            }
            Message::OpenTrackProps(id) => {
                self.modal_drag_offset = Point::ORIGIN;
                if let Some(t) = self.tracks.iter().find(|t| t.id == id) {
                    // 1. Сначала извлекаем и клонируем всё, что нужно
                    let backup = t.as_ref().clone();
                    let draft = backup.props.clone();
                    
                    // 2. Ссылка `t` больше не используется, теперь можно менять self
                    self.push_undo(t!("msg_0045"));
                    self.track_backup = Some(backup);
                    self.modal = Some(Modal::TrackProps { track_id: id, draft });
                }
            }
            Message::ApplyTrackProps => {
                self.modal_drag_offset = Point::ORIGIN;
                self.track_backup = None;
                self.modal = None;
            }
            Message::CloseTrackProps => {
                self.modal_drag_offset = Point::ORIGIN;
                if let Some(backup) = self.track_backup.take() {
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == backup.id) {
                        *t = Arc::new(backup);
                        self.tracks_cache.clear();
                    }
                }
                self.show_color_picker = None;
                self.modal = None;
            }
            Message::ToggleTrackSaveMenu => {
                self.show_track_save_menu = !self.show_track_save_menu;
                self.show_wp_save_menu = false;
            }
            Message::SimplifyTracks => {
                let ids = self.selection.tracks.clone();
                if ids.is_empty() { return; }

                // 1. ОБЯЗАТЕЛЬНО сохраняем состояние ДО изменений
                self.push_undo(t!("msg_0046"));
                
                // 2. Применяем фильтр
                for id in ids {
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                        domain::simplify::simplify_track(Arc::make_mut(t), self.simplify_tolerance);
                    }
                }
                
                // 3. Обновляем UI
                self.tracks_cache.clear();
                self.status = format!("{} {} {})", t!("msg_0231"), self.simplify_tolerance, t!("msg_0221"));
            }
            Message::MergeTracks => {
                // Нужно минимум 2 выделенных трека
                if self.selection.tracks.len() < 2 { return; }

                // «Первый» = самый верхний в списке (порядок `self.tracks`).
                // HashSet не хранит порядок, поэтому ориентируемся на позицию в UI.
                let Some(target_id) = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| t.id)
                else { return };

                self.push_undo(t!("msg_0047"));

                // Собираем сегменты и id остальных выделенных в порядке списка
                let mut extra_segments: Vec<domain::TrackSegment> = Vec::new();
                let mut other_ids: Vec<u64> = Vec::new();
                for t in &self.tracks {
                    if t.id == target_id { continue; }
                    if !self.selection.tracks.contains(&t.id) { continue; }
                    for seg in &t.segments {
                        extra_segments.push(domain::TrackSegment { points: seg.points.clone() });
                    }
                    other_ids.push(t.id);
                }

                if extra_segments.is_empty() && other_ids.is_empty() {
                    // Защита: если выделены только id, которых нет в списке
                    return;
                }

                // Дописываем сегменты в целевой трек, помечаем как New
                if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == target_id) {
                    let t_m = Arc::make_mut(t_mut);
                    for seg in extra_segments {
                        t_m.segments.push(seg);
                    }
                    // После объединения источник-файл больше не соответствует содержимому
                    t_m.source = domain::FileSource::New;
                    domain::recompute_track_stats(t_m);
                }

                // Удаляем остальные
                self.tracks.retain(|t| !other_ids.contains(&t.id));

                // Выделение — только целевой трек
                self.selection.tracks.clear();
                self.selection.tracks.insert(target_id);

                self.tracks_cache.clear();
                self.status = format!("{} {}", t!("msg_0232"), other_ids.len() + 1);
            }
            
            Message::SplitTrackBySegments => {
                // Только для одного выделенного трека с >1 сегментов.
                if self.selection.tracks.len() != 1 { return; }
                let id = *self.selection.tracks.iter().next().unwrap();

                let Some(track_arc) = self.tracks.iter().find(|t| t.id == id).cloned() else { return };
                let track = track_arc.as_ref();
                let n_segments = track.segments.len();
                if n_segments <= 1 { return; }

                self.push_undo(t!("msg_0048"));

                let original_name = track.name.clone();
                let color = track.color;
                let visible = track.visible;
                let base_props = track.props.clone();
                let segments: Vec<_> = track.segments.clone();
                let pos = self.tracks.iter().position(|t| t.id == id).unwrap();

                let mut new_tracks: Vec<Arc<domain::Track>> = Vec::new();
                for (i, seg) in segments.into_iter().enumerate() {
                    let new_name = format!("{}_Segment_{}", original_name, i + 1);
                    let mut new_props = base_props.clone();
                    new_props.description = new_name.clone();

                    // Первый сегмент сохраняет исходный id (rename), остальные — новые.
                    // Source = New для всех: файл-источник больше не соответствует ни
                    // одному треку после разбиения. Save As — правильный путь.
                    let mut nt = domain::Track {
                        id: if i == 0 { id } else { self.next_id },
                        name: new_name,
                        color,
                        visible,
                        segments: vec![seg],
                        source: domain::FileSource::New,
                        props: new_props,
                        cached_points: 0,
                        cached_distance_m: 0.0,
                    };
                    if i != 0 { self.next_id += 1; }
                    domain::recompute_track_stats(&mut nt);
                    new_tracks.push(Arc::new(nt));
                }

                // Вставляем разбиение на место оригинала, порядок сохраняется.
                self.tracks.remove(pos);
                for (i, t) in new_tracks.into_iter().enumerate() {
                    self.tracks.insert(pos + i, t);
                }

                self.selection.tracks.clear();
                self.tracks_cache.clear();
                self.status = format!("{} {} {}", t!("msg_0233"), n_segments, t!("msg_0234"));
            }
            Message::DeleteSelectedPoints => {
                // Защита: не удаляем, если открыто модальное окно (например, редактируется текст)
                if self.modal.is_some() { return; }
                // Защита: не удаляем, если не активен инструмент выделения точек
                if self.active_tool != crate::Tool::SelectPoints { return; }
                // Защита: нечего удалять
                if self.selection.points.is_empty() { return; }
                
                self.push_undo(t!("msg_0049"));
                
                // Группируем выделенные точки по (track_id, seg_idx)
                let mut by_track: std::collections::HashMap<u64, Vec<(usize, Vec<usize>)>> = std::collections::HashMap::new();
                for (track_id, seg_idx, pt_idx) in &self.selection.points {
                    let segs = by_track.entry(*track_id).or_default();
                    if let Some(entry) = segs.iter_mut().find(|(s, _)| s == seg_idx) {
                        entry.1.push(*pt_idx);
                    } else {
                        segs.push((*seg_idx, vec![*pt_idx]));
                    }
                }
                
                for (track_id, seg_updates) in by_track {
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                        let t_m = Arc::make_mut(t);
                        for (seg_idx, mut indices) in seg_updates {
                            if let Some(seg) = t_m.segments.get_mut(seg_idx) {
                                // Удаляем в обратном порядке, чтобы не сбить индексы
                                indices.sort_unstable_by(|a, b| b.cmp(a));
                                for &idx in &indices {
                                    if idx < seg.points.len() {
                                        seg.points.remove(idx);
                                    }
                                }
                            }
                        }
                        // Удаляем пустые сегменты (0 точек)
                        t_m.segments.retain(|s| !s.points.is_empty());
                        domain::recompute_track_stats(t_m);
                    }
                }
                
                self.selection.points.clear();
                self.last_point_clicked = None;
                self.tracks_cache.clear();
                self.status = t!("msg_0050").into();
            }
            Message::FitTrackToScreen => {
                let ids: Vec<u64> = if !self.selection.tracks.is_empty() {
                    self.selection.tracks.iter().copied().collect()
                } else {
                    self.tracks.iter().filter(|t| t.visible).map(|t| t.id).collect()
                };

                let mut min_x = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                let mut found = false;

                for id in ids {
                    if let Some(t) = self.tracks.iter().find(|t| t.id == id) {
                        for seg in &t.segments {
                            for pt in &seg.points {
                                let (mx, my) = merc_of(pt.lon, pt.lat);
                                if mx < min_x { min_x = mx; }
                                if mx > max_x { max_x = mx; }
                                if my < min_y { min_y = my; }
                                if my > max_y { max_y = my; }
                                found = true;
                            }
                        }
                    }
                }

                if found {
                    let w = (max_x - min_x).max(0.001);
                    let h = (max_y - min_y).max(0.001);
                    let sz = self.last_bounds.get();
                    let vw = sz.width as f64;
                    let vh = sz.height as f64;

                    // Паддинг 100px на узкую сторону
                    let (pad_x, pad_y) = if vw < vh { (100.0, 0.0) } else { (0.0, 100.0) };

                    let scale_x = w / (vw - pad_x).max(1.0);
                    let scale_y = h / (vh - pad_y).max(1.0);

                    self.scale = scale_x.max(scale_y).max(0.1).min(10000.0);
                    self.cx = (min_x + max_x) / 2.0;
                    self.cy = (min_y + max_y) / 2.0;

                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.status = format!("Fit: {} {:.2} {}", t!("msg_0215"),  self.scale, t!("msg_0216"));
                } else {
                    self.status = t!("msg_0051").into();
                }
            }
            Message::FreehandReleased => {
                self.finish_freehand();
            }
            Message::DeleteTrackById(id) => {
                self.push_undo(t!("msg_0052"));
                self.tracks.retain(|t| t.id != id);
                self.selection.tracks.remove(&id);
                self.tracks_cache.clear();
            }
            Message::FitTrackById(id) => {
                self.selection.tracks.clear();
                self.selection.tracks.insert(id);
                self.update_tracks(Message::FitTrackToScreen);
            }
            Message::SplitTrackAt(id, seg_idx, pt_idx) => {
                self.push_undo(t!("msg_0053"));

                let Some(track_arc) = self.tracks.iter().find(|t| t.id == id).cloned() else { return };
                let track = track_arc.as_ref();

                // Собираем плоский список точек
                let mut all_points: Vec<domain::TrackPoint> = Vec::new();
                for seg in &track.segments {
                    all_points.extend(seg.points.iter().cloned());
                }

                // Глобальный индекс
                let mut global_idx = 0;
                for s in 0..seg_idx {
                    global_idx += track.segments[s].points.len();
                }
                global_idx += pt_idx;

                if global_idx == 0 || global_idx >= all_points.len() - 1 { return; }

                let part1: Vec<_> = all_points[..=global_idx].to_vec();
                let part2: Vec<_> = all_points[global_idx..].to_vec();

                let new_name = format!("{}_Сегмент2", track.name);
                let new_color = track.color;
                let new_props = track.props.clone();

                if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == id) {
                    let t = Arc::make_mut(t_mut);
                    t.segments = vec![domain::TrackSegment { points: part1 }];
                    domain::recompute_track_stats(t);
                }

                let mut new_track = domain::Track {
                    id: self.next_id,
                    name: new_name,
                    color: new_color,
                    visible: true,
                    segments: vec![domain::TrackSegment { points: part2 }],
                    source: domain::FileSource::New,
                    props: new_props,
                    cached_points: 0,
                    cached_distance_m: 0.0,
                };
                domain::recompute_track_stats(&mut new_track);
                self.next_id += 1;
                self.tracks.push(Arc::new(new_track));

                self.tracks_cache.clear();
                self.status = t!("msg_0054").into();
            }
            _ => {}
        }
    }

    // =========================================================================
    // 4. Модальное окно трека
    // =========================================================================
    fn update_track_modal(&mut self, m: Message) {
        match m {
            Message::DraftDescChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.description = s;
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftDateChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.date_str = s;
                    draft.description = build_template(draft);
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftCallsignChanged(cs) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.callsign = cs.clone();
                    draft.line_color = match cs.as_str() {
                        "Veter" => [255, 0, 255],
                        "BPLA" => [0, 255, 255],
                        "Bort" => [255, 255, 255],
                        "Zadacha" => [0, 0, 0],
                        _ => draft.line_color,
                    };
                    draft.line_width = if cs == "BPLA" { 1.0 } else { 2.0 };
                    
                    let is_lisa_like = matches!(cs.as_str(), "Lisa" | "Avtonom" | "Kinolog" | "Pegas");
                    let forbidden = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];
                    if is_lisa_like && forbidden.contains(&draft.line_color) {
                        draft.line_color = [255, 0, 0];
                    }
                    draft.description = build_template(draft);
                    tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftNum1Changed(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    if let Ok(n) = s.parse::<u32>() { draft.num1 = n; draft.description = build_template(draft); tid = Some(*track_id); }
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftNum2Changed(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.num2 = s.parse::<u32>().ok(); draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftSuffixChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.suffix = s; draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftDateToday => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.date_str = date_offset(0); draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftDateYesterday => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.date_str = date_offset(-1); draft.description = build_template(draft); tid = Some(*track_id);
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    // Guard активен только когда Description воспроизводится через шаблон —
                    // то есть все поля (callsign, num, date) заполнены и в правильном формате.
                    let template_ready = build_template(draft) == draft.description;

                    draft.line_color = if template_ready {
                        match draft.callsign.as_str() {
                            "Lisa" | "Avtonom" | "Kinolog" | "Pegas" => {
                                let forbidden = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];
                                if forbidden.contains(&c) { [255, 0, 0] } else { c }
                            }
                            "Veter"   => [255, 0, 255],   // только magenta
                            "BPLA"    => [0, 255, 255],   // только cyan
                            "Bort"    => [255, 255, 255], // только white
                            "Zadacha" => [0, 0, 0],       // только black
                            _ => c,                       // неизвестный/пустой callsign — свободно
                        }
                    } else {
                        c  // не соответствует шаблону — можно любой цвет
                    };
                    tid = Some(*track_id);
                }
                self.show_color_picker = None;
                if let Some(id) = tid { self.update_track_preview(id); }
            }

            Message::DraftFillColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    draft.fill_color = c;
                    tid = Some(*track_id);
                }
                self.show_color_picker = None;   // ← ЭТО, скорее всего, отсутствует
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftWidthChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal {
                    if let Ok(w) = s.parse::<f32>() { draft.line_width = w; tid = Some(*track_id); }
                }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftStyleChanged(s) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.line_style = s; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftTypeChanged(t) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.poly_type = t; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftFillTypeChanged(t) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.fill_type = t; tid = Some(*track_id); }
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::ToggleColorPicker(kind) => {
                self.show_color_picker = if self.show_color_picker == Some(kind) {
                    None
                } else {
                    Some(kind)
                };
            }
            _ => {}
        }
    }

    // =========================================================================
    // 5. Точки (Логика списка)
    // =========================================================================
    fn update_waypoints(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::SelectWaypoint(id) => {
                self.search_focused = false;
                let now = Instant::now();
                let is_double_click = self.last_waypoint_click.map_or(false, |(last_id, last_time)| {
                    last_id == id && now.duration_since(last_time).as_millis() < 300
                });

                if is_double_click {
                    self.modal_drag_offset = Point::ORIGIN;
                    self.last_waypoint_click = None;
                    if let Some(w) = self.waypoints.iter().find(|w| w.id == id) {
                        self.waypoint_backup = Some(w.as_ref().clone());
                        self.modal = Some(Modal::WaypointProps { 
                            waypoint_id: id, 
                            draft: w.as_ref().clone(),
                            coord_input: format!("{:.6} {:.6}", w.lat, w.lon),
                            is_new: false,
                        });
                    }
                } else {
                    self.last_waypoint_click = Some((id, now));
                    let ctrl = self.modifiers.control() || self.modifiers.command();
                    let shift = self.modifiers.shift();

                    if shift {
                        if let Some(last_id) = self.last_waypoint_clicked {
                            if let (Some(start), Some(end)) = (
                                self.waypoints.iter().position(|w| w.id == last_id),
                                self.waypoints.iter().position(|w| w.id == id),
                            ) {
                                let (min, max) = if start < end { (start, end) } else { (end, start) };
                                for i in min..=max {
                                    self.selection.waypoints.insert(self.waypoints[i].id);
                                }
                            }
                        } else {
                            self.selection.waypoints.insert(id);
                        }
                    } else if ctrl {
                        if !self.selection.waypoints.remove(&id) {
                            self.selection.waypoints.insert(id);
                        }
                    } else {
                        self.selection.waypoints.clear();
                        self.selection.tracks.clear();
                        self.selection.waypoints.insert(id);
                    }
                    self.last_waypoint_clicked = Some(id);
                    self.tracks_cache.clear();
                }
            }
            Message::ToggleWaypointVisible(id, v) => {
                if self.modifiers.alt() {
                    // Solo-режим: показываем только кликнутую, остальные скрываем
                    self.push_undo(t!("msg_0055"));
                    for w in &mut self.waypoints {
                        let should_be_visible = w.id == id;
                        if w.visible != should_be_visible {
                            Arc::make_mut(w).visible = should_be_visible;
                        }
                    }
                } else {
                    // Обычное переключение
                    if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == id) {
                        if w.visible != v {
                            Arc::make_mut(w).visible = v;
                        }
                    }
                }
                self.tracks_cache.clear();
            }
            Message::ToggleAllWaypointsVisible => {
                let all_visible = self.waypoints.iter().all(|w| w.visible);
                for w in &mut self.waypoints {
                    Arc::make_mut(w).visible = !all_visible; 
                }
                self.tracks_cache.clear();
            }
            Message::DeleteSelectedWaypoints => {
                self.push_undo(t!("msg_0056")); 
                let ids = self.selection.waypoints.clone();
                self.waypoints.retain(|w| !ids.contains(&w.id));
                self.selection.waypoints.clear();
                self.selection.tracks.clear();
                self.tracks_cache.clear();
            }
            Message::OpenWaypointProps(id) => {
                self.modal_drag_offset = Point::ORIGIN;
                if let Some(w) = self.waypoints.iter().find(|w| w.id == id) {
                    // 1. Сначала извлекаем
                    let backup = w.as_ref().clone();
                    let coord_input = format!("{:.6} {:.6}", backup.lat, backup.lon);
                    
                    // 2. Теперь можно менять self
                    self.push_undo(t!("msg_0057"));
                    self.waypoint_backup = Some(backup.clone());
                    self.modal = Some(Modal::WaypointProps { 
                        waypoint_id: id, 
                        draft: backup,
                        coord_input,
                        is_new: false,
                    });
                }
            }
            Message::AddNewWaypoint => {
                self.modal_drag_offset = Point::ORIGIN;
                self.push_undo(t!("msg_0058"));
                let (lon, lat) = lonlat_of(self.cx, self.cy);
                let new_wp = domain::Waypoint {
                    id: self.next_id,
                    name: t!("msg_0059").into(),
                    lon,
                    lat,
                    ele: None, time: None, symbol: None, description: None,
                    visible: true,
                    source: domain::FileSource::New,
                    text_color: [0, 0, 0],
                    bg_color: [0, 255, 255],
                };
                self.next_id += 1;
                self.waypoints.push(Arc::new(new_wp.clone()));
                self.waypoint_backup = Some(new_wp.clone());
                self.modal = Some(Modal::WaypointProps {
                    waypoint_id: new_wp.id,
                    draft: new_wp,
                    coord_input: format!("{:.6} {:.6}", lat, lon),
                    is_new: true,
                });
                self.tracks_cache.clear();
            }
            Message::ApplyWaypointProps => {
                self.modal_drag_offset = Point::ORIGIN;
                self.waypoint_backup = None;
                self.modal = None;
                self.waypoint_crosshair = None;
            }
            Message::CloseWaypointProps => {
                self.modal_drag_offset = Point::ORIGIN;
                if let Some(Modal::WaypointProps { waypoint_id, is_new, .. }) = &self.modal {
                    if *is_new {
                        let id = *waypoint_id;
                        self.waypoints.retain(|w| w.id != id);
                    }
                }
                if let Some(backup) = self.waypoint_backup.take() {
                    if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == backup.id) {
                        *w = Arc::new(backup);
                    }
                }
                self.tracks_cache.clear();
                self.show_color_picker = None;
                self.modal = None;
                self.waypoint_crosshair = None;
            }
            Message::ToggleWpSaveMenu => {
                self.show_wp_save_menu = !self.show_wp_save_menu;
                self.show_track_save_menu = false;
            }
            Message::ClearWaypointCrosshair => {
                self.waypoint_crosshair = None;
                self.tracks_cache.clear();
            }
            Message::CenterOnWaypoint(id) => {
                if let Some(w) = self.waypoints.iter().find(|w| w.id == id) {
                    let (mx, my) = merc_of(w.lon, w.lat);
                    self.cx = mx;
                    self.cy = my;
                    self.waypoint_crosshair = Some((w.lon, w.lat));
                    
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.maybe_update_declination(true);

                    return Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        },
                        |_| Message::ClearWaypointCrosshair,
                    );
                }
            }
            Message::MeasureWaypoints => {
                if self.selection.waypoints.len() == 2 {
                    let mut iter = self.selection.waypoints.iter().copied();
                    let id1 = iter.next().unwrap();
                    let id2 = iter.next().unwrap();

                    // last_waypoint_clicked — это "вторая" точка (куда смотрим)
                    let (id_from, id_to) = if self.last_waypoint_clicked == Some(id1) {
                        (id2, id1)
                    } else {
                        (id1, id2)
                    };

                    if let (Some(w1), Some(w2)) = (
                        self.waypoints.iter().find(|w| w.id == id_from),
                        self.waypoints.iter().find(|w| w.id == id_to)
                    ) {
                        let dist_m = domain::haversine_m(w1.lat, w1.lon, w2.lat, w2.lon);
                        let true_az = crate::util::geo::true_bearing(w1.lat, w1.lon, w2.lat, w2.lon);
                        
                        let mut mag_az = true_az - self.declination;
                        if mag_az < 0.0 { mag_az += 360.0; }
                        if mag_az >= 360.0 { mag_az -= 360.0; }

                        self.modal = Some(Modal::WaypointMeasure {
                            from_name: w1.name.clone(),
                            from_lat: w1.lat, from_lon: w1.lon,
                            to_name: w2.name.clone(),
                            to_lat: w2.lat, to_lon: w2.lon,
                            dist_m,
                            mag_az,
                        });
                    }
                }
            }
            Message::CloseWaypointMeasure => {
                self.modal = None;
            }
            Message::DeleteWaypointById(id) => {
                self.push_undo(t!("msg_0060"));
                self.waypoints.retain(|w| w.id != id);
                self.selection.waypoints.remove(&id);
                self.tracks_cache.clear();
            }
            Message::AddWaypointAt(lon, lat) => {
                let new_wp = domain::Waypoint {
                    id: self.next_id,
                    name: format!("Новая точка{}", self.next_id),
                    lon, lat,
                    ele: None, time: None, symbol: None, description: None,
                    visible: true,
                    source: domain::FileSource::New,
                    text_color: [0, 0, 0],
                    bg_color: [0, 255, 255],
                };
                self.next_id += 1;
                self.push_undo(t!("msg_0058"));
                self.waypoints.push(Arc::new(new_wp.clone()));
                self.waypoint_backup = Some(new_wp.clone());
                self.modal = Some(Modal::WaypointProps {
                    waypoint_id: new_wp.id,
                    draft: new_wp,
                    coord_input: format!("{:.6} {:.6}", lat, lon),
                    is_new: true,
                });
                self.tracks_cache.clear();
            }
            Message::OpenWaypointProjection(id) => {
                if self.waypoints.iter().any(|w| w.id == id) {
                    self.modal = Some(Modal::WaypointProjection {
                        from_id: id,
                        azimuth_input: "0".to_string(),
                        distance_input: "100".to_string(),
                    });
                }
            }
            Message::CloseWaypointProjection => {
                self.modal = None;
            }
            Message::ProjectAzimuthChanged(s) => {
                if let Some(Modal::WaypointProjection { azimuth_input, .. }) = &mut self.modal {
                    *azimuth_input = s;
                }
            }
            Message::ProjectDistanceChanged(s) => {
                if let Some(Modal::WaypointProjection { distance_input, .. }) = &mut self.modal {
                    *distance_input = s;
                }
            }

            Message::ExecuteProjection => {
                if let Some(Modal::WaypointProjection {
                    from_id,
                    azimuth_input,
                    distance_input,
                }) = &self.modal {
                    let from_id = *from_id;
                    let az = azimuth_input.parse::<f64>().ok();
                    let dist = distance_input.parse::<f64>().ok();

                    if let (Some(az), Some(dist)) = (az, dist) {
                        if !az.is_finite() || !dist.is_finite() || dist < 0.0 {
                            self.status = t!("msg_0061").into();
                            return Task::none();
                        }

                        if let Some(w) = self.waypoints.iter().find(|w| w.id == from_id) {
                            // Средний радиус Земли, метры
                            const EARTH_RADIUS: f64 = 6_371_008.8;

                            // Истинный азимут в радианах
                            let bearing = (az + self.declination).to_radians();

                            // Исходные координаты
                            let lat1 = w.lat.to_radians();
                            let lon1 = w.lon.to_radians();

                            // Угловое расстояние
                            let angular_distance = dist / EARTH_RADIUS;

                            // Новая широта
                            let lat2 = (
                                lat1.sin() * angular_distance.cos()
                                + lat1.cos() * angular_distance.sin() * bearing.cos()
                            ).clamp(-1.0, 1.0).asin();

                            // Новая долгота
                            let lon2 = lon1 + (
                                bearing.sin() * angular_distance.sin() * lat1.cos()
                            ).atan2(
                                angular_distance.cos() - lat1.sin() * lat2.sin()
                            );

                            // Перевод обратно в градусы
                            let new_lat = lat2.to_degrees();
                            let new_lon = (lon2.to_degrees() + 540.0)
                                .rem_euclid(360.0) - 180.0;

                            let new_wp = domain::Waypoint {
                                id: self.next_id,
                                name: format!("{}_proj", w.name),
                                lon: new_lon,
                                lat: new_lat,
                                ele: None,
                                time: None,
                                symbol: None,
                                description: None,
                                visible: true,
                                source: domain::FileSource::New,
                                text_color: w.text_color,
                                bg_color: w.bg_color,
                            };

                            self.next_id += 1;
                            self.push_undo(t!("msg_0062"));
                            self.waypoints.push(Arc::new(new_wp));
                            self.modal = None;
                            self.tracks_cache.clear();

                            self.status = format!(
                                "Создана точка проекции ({} м, {:.0}°)",
                                dist, az
                            );
                        }
                    } else {
                        self.status = t!("msg_0063").into();
                    }
                }
            }

            _ => {}
        }
        Task::none()        
    }

    // =========================================================================
    // 6. Модальное окно точки
    // =========================================================================
    fn update_wp_modal(&mut self, m: Message) {
        match m {
            Message::DraftWpNameChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.name = s; tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpDescChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.description = Some(s); tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpEleChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.ele = s.parse::<f64>().ok(); tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }

            Message::DraftWpBgColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.bg_color = c;
                    tid = Some(*waypoint_id);
                }
                self.show_color_picker = None;   // ← добавить, если нет
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            
            Message::DraftWpTextColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.text_color = c;    // ← вот здесь читается поле
                    tid = Some(*waypoint_id);
                }
                self.show_color_picker = None;
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpCoordsChanged(s) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, coord_input, .. }) = &mut self.modal {
                    *coord_input = s.clone();
                    if let Some((lat, lon)) = parse_coords(&s, self.grid.as_ref(), self.declination) {
                        if lat.abs() <= 90.0 && lon.abs() <= 180.0 {
                            draft.lat = lat;
                            draft.lon = lon;
                            draft.ele = None;
                            tid = Some(*waypoint_id);
                        }
                    }
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            _ => {}
        }
    }
    // =========================================================================
    // 7. Сохранение файлов
    // =========================================================================
    fn update_save(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::SaveTracksPlt => {
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "track".to_string());

            let dialog = self.save_dialog_with_dir(
                &format!("{}.plt", name),
                "OziExplorer Track",
                &["plt"],
            );
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksPltPicked);
            }
            
            Message::SaveTracksPltPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter()
                    .filter(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| t.as_ref()).collect();
                let sel_wps: Vec<_> = self.waypoints.iter()
                    .filter(|w| self.selection.waypoints.contains(&w.id))
                    .map(|w| w.as_ref()).collect();
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                    // remember_save_dir НЕ вызывать
                }
            }
            
            Message::SaveTracksPltPicked(None) => {}
            
            Message::SaveTracksGpx => {
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "tracks".to_string());
    
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{}.gpx", name))
                    .add_filter("GPX", &["gpx"]);
                
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksGpxPicked);
            }
            Message::SaveTracksGpxPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter().filter(|t| self.selection.tracks.contains(&t.id)).map(|t| t.as_ref()) .collect();
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).map(|w| w.as_ref()).collect();
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksGpxPicked(None) => {}
            
            Message::SaveTracksMs => {
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "tracks".to_string());

                let dialog = self.save_dialog_with_dir(
                    &format!("{}.ms", name),
                    "Guru Maps",
                    &["ms"],
                );
                return Task::perform(
                    async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) },
                    Message::SaveTracksMsPicked,
                );
            }
            
            Message::SaveTracksMsPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter().filter(|t| self.selection.tracks.contains(&t.id)).map(|t| t.as_ref()).collect();
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).map(|w| w.as_ref()).collect();
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksMsPicked(None) => {}
            
            Message::SaveTracksKml => {
                let name = self.tracks.iter()
                    .find(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| if t.props.description.is_empty() { t.name.clone() } else { t.props.description.clone() })
                    .unwrap_or_else(|| "tracks".to_string());

                let dialog = self.save_dialog_with_dir(
                    &format!("{}.kml", name),
                    "KML",
                    &["kml"],
                );
                return Task::perform(
                    async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) },
                    Message::SaveTracksKmlPicked,
                );
            }
            Message::SaveTracksKmlPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter()
                    .filter(|t| self.selection.tracks.contains(&t.id))
                    .map(|t| t.as_ref()).collect();
                let sel_wps: Vec<_> = self.waypoints.iter()
                    .filter(|w| self.selection.waypoints.contains(&w.id))
                    .map(|w| w.as_ref()).collect();
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveTracksKmlPicked(None) => {}
            
            Message::SaveWaypointsWptNew => {
                let date_str = date_offset(0).replace("-", "");
                let default_name = format!("Waypoints_{}.wpt", date_str);

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter("OziExplorer Waypoints", &["wpt"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(
                    async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) },
                    Message::SaveWaypointsWptNewPicked,
                );
            }
            
            Message::SaveWaypointsWptNewPicked(None) => {}            

            Message::SaveWaypointsWptAppend => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("OziExplorer Waypoints", &["wpt"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(
                    async move { dialog.pick_file().await.map(|h| h.path().to_path_buf()) },
                    Message::SaveWaypointsWptAppendPicked,
                );
            }


            Message::SaveWaypointsWptAppendPicked(Some(path)) => {
                let mut all_wps = Vec::new();
                if let Ok(content) = io::load_user_file(&path, &mut self.next_id) {
                    all_wps.extend(content.waypoints);
                }
                for w in &self.waypoints {
                    if self.selection.waypoints.contains(&w.id) {
                        all_wps.push(w.as_ref().clone());
                    }
                }
                let refs: Vec<_> = all_wps.iter().collect();
                if let Err(e) = io::save_any(&[], &refs, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Добавлено в {}", path.display());
                }
            }
            Message::SaveWaypointsWptAppendPicked(None) => {}

            Message::SaveTrackProps => {
                if let Some(Modal::TrackProps { draft, .. }) = &self.modal {
                    let default_name = format!("{}.plt", draft.description);
                    let mut dialog = rfd::AsyncFileDialog::new()
                        .set_file_name(&default_name)
                        .add_filter("OziExplorer Track", &["plt"]);

                    if let Some(kit) = &self.last_kit {
                        let tracks_dir = kit.join("10-Tracks");
                        if tracks_dir.is_dir() {
                            dialog = dialog.set_directory(&tracks_dir);
                        } else if kit.is_dir() {
                            dialog = dialog.set_directory(kit);
                        }
                    }

                    return Task::perform(
                        async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) },
                        Message::SaveTrackPropsPicked,
                    );
                }
            }
            
            Message::SaveTrackPropsPicked(Some(path)) => {
                self.modal_drag_offset = Point::ORIGIN;
                let track_id = if let Some(Modal::TrackProps { track_id, .. }) = &self.modal {
                    *track_id
                } else {
                    return Task::none();
                };

                let save_result = if let Some(t) = self.tracks.iter().find(|t| t.id == track_id) {
                    io::save_plt(t, &path)
                } else {
                    Err(t!("msg_0064").to_string())
                };

                match save_result {
                    Err(e) => {
                        self.status = format!("Ошибка сохранения: {e}");
                    }
                    Ok(_) => {
                        self.push_undo(t!("msg_0065"));
                        self.status = format!("Сохранено в {}", path.display());
                        // remember_save_dir НЕ вызывать
                        if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                            let t_m = Arc::make_mut(t_mut);
                            t_m.source = domain::FileSource::File {
                                path: path.clone(),
                                read_only: false
                            };
                            t_m.name = t_m.props.description.clone();
                        }
                        self.modal = None;
                        self.track_backup = None;
                    }
                }
            }
            
            Message::SaveTrackPropsPicked(None) => {}
            _ => {}
        }
        Task::none()
    }

    // =========================================================================
    // 8. Системные операции
    // =========================================================================
    fn update_system(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::PickKit => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::KitPicked,
                );
            }
            Message::KitPicked(Some(path)) => {
                if self.apply_kit(load_kit(&path)) {
                    self.last_kit = Some(path.clone());
                    session::save_last_kit(&path);
                }
            }
            Message::KitPicked(None) => {}
            
            Message::PickTrack => {
                let dir = self.last_open_dir.clone();
                return Task::perform(
                    async move {
                        let mut dialog = rfd::AsyncFileDialog::new()
                            .add_filter("Track/Points", &["gpx", "plt", "ms", "wpt", "kml"]);
                        if let Some(d) = dir {
                            dialog = dialog.set_directory(&d);
                        }
                        dialog.pick_files().await
                    },
                    |files| Message::TracksPicked(
                        files.map(|fs| fs.into_iter().map(|f| f.path().to_path_buf()).collect())
                    ),
                );
            }
            Message::TracksPicked(Some(paths)) => {
                if let Some(first) = paths.first() {
                    if let Some(parent) = first.parent() {
                        self.last_open_dir = Some(parent.to_path_buf());
                    }
                }
                for path in paths {
                    match io::load_user_file(&path, &mut self.next_id) {
                        Ok(content) => {
                            self.tracks.extend(content.tracks.into_iter().map(Arc::new));
                            self.waypoints.extend(content.waypoints.into_iter().map(Arc::new));
                            let mut s = format!("Загружено: {}", path.file_name().unwrap_or_default().to_string_lossy());
                            if !content.notes.is_empty() {
                                s += &format!(" (предупреждения: {})", content.notes.join("; "));
                            }
                            self.status = s;
                        }
                        Err(e) => {
                            self.status = format!("Ошибка загрузки: {e}");
                        }
                    }
                }
                self.tracks_cache.clear();
            }
            Message::TracksPicked(None) => {}

            Message::WindowCloseRequested => {
                save_session(self);
                return iced::exit();
            }
            Message::ModifiersChanged(mods) => {
                self.modifiers = mods;
            }
            Message::OsmTileLoaded(source, id, result) => {
                // Ответ пришёл от источника, с которого пользователь уже ушёл.
                if Some(source) != self.online_basemap() {
                    // Снимаем pending, чтобы не блокировало будущие запросы этого тайла.
                    self.osm_cache.unmark_pending(id);
                    return Task::none();
                }

                self.osm_cache.unmark_pending(id);
                match result {
                    Ok(bytes) => {
                        let handle = iced::widget::image::Handle::from_bytes(bytes);
                        self.osm_cache.insert(id, Some(handle));
                    }
                    Err(e) => {
                        let cnt = self.tile_fail_count.entry(id).or_insert(0);
                        *cnt += 1;
                        if *cnt <= 3 {
                            eprintln!("tile {:?} failed (try {}): {}", id, cnt, e);
                        } else {
                            self.osm_cache.insert(id, None);
                        }
                    }
                }
                self.osm_canvas_cache.clear();
            }
            Message::RefreshSystemTheme => {
                // User-selected themes are pinned; OS notifications must not override them.
                let (theme, resolved) = theme_pair_for(&self.ui_theme, self.current_theme);
                self.theme = theme;
                self.resolved_theme = resolved;
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
            }
            Message::AutoSaveTick => {
                save_session(self);
                // Статус не трогаем — чтобы не мигала надпись каждые 5 минут.
                // Если хочется обратной связи, раскомментируйте:
                // self.status = format!("Автосохранение: {}", chrono::Local::now().format("%H:%M"));
            }
            Message::None => {}
            _ => {}
        }
        Task::none()
    }
    
    // The map selection (0-based segment number) is the persistent source of truth.
    // TrackList stores a 1-based view of the same selected points for its UI.
    fn sync_tracklist_selection(&mut self) {
        let Some(Modal::TrackList(state)) = &self.modal else { return };
        let id = state.track_id;
        let selected = state.selected_points.clone();
        let last = state.last_clicked;

        // Do not remove selections belonging to other tracks.
        self.selection.points.retain(|(track_id, _, _)| *track_id != id);
        for (sect, pt_idx) in selected {
            if sect > 0 {
                self.selection.points.insert((id, sect - 1, pt_idx));
            }
        }
        if !state.selected_points.is_empty() {
            // Required for the SelectPoints canvas to display selected diamonds.
            self.selection.tracks.insert(id);
        }
        if let Some((sect, pt_idx)) = last.filter(|(sect, _)| *sect > 0) {
            self.last_point_clicked = Some((id, sect - 1, pt_idx));
        } else if self.last_point_clicked.is_some_and(|(track_id, _, _)| track_id == id) {
            self.last_point_clicked = None;
        }
    }

    // Structural changes shift segment/point indices, so stale selections must go.
    fn clear_track_point_selection(&mut self, id: u64) {
        self.selection.points.retain(|(track_id, _, _)| *track_id != id);
        if self.last_point_clicked.is_some_and(|(track_id, _, _)| track_id == id) {
            self.last_point_clicked = None;
        }
    }

    fn update_track_list(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::OpenTrackList(id) => {
                if !matches!(self.modal, Some(Modal::TrackList(_))) {
                    self.modal_drag_offset = Point::ORIGIN;
                }
                if let Some(t) = self.tracks.iter().find(|t| t.id == id) {
                    let mut rows = Vec::new();
                    let mut pid = 1;
                    
                    for (sect_idx, seg) in t.segments.iter().enumerate() {
                        let mut prev_pt: Option<&domain::TrackPoint> = None;
                        for (pt_idx, pt) in seg.points.iter().enumerate() {
                            let mut dist_m = 0.0;
                            let mut kph = 0.0;
                            if let Some(prev) = prev_pt {
                                dist_m = domain::haversine_m(prev.lat, prev.lon, pt.lat, pt.lon);
                                if let (Some(t1), Some(t2)) = (prev.time, pt.time) {
                                    let dt = t2 - t1;
                                    if dt > 0.0 {
                                        kph = (dist_m / dt) * 3.6;
                                    }
                                }
                            }
                            rows.push(TrackRowData {
                                pid, sect: sect_idx + 1, pt_idx,
                                lat: pt.lat, lon: pt.lon, ele: pt.ele, time: pt.time,
                                dist_m, kph,
                            });
                            pid += 1;
                            prev_pt = Some(pt);
                        }
                    }

                    // Reuse map selections instead of starting with an empty list.
                    let selected_points: HashSet<(usize, usize)> = self.selection.points
                        .iter()
                        .filter(|(track_id, _, _)| *track_id == id)
                        .map(|(_, seg_idx, pt_idx)| (*seg_idx + 1, *pt_idx))
                        .collect();
                    let first_selected = rows.iter().enumerate()
                        .find(|(_, r)| selected_points.contains(&(r.sect, r.pt_idx)))
                        .map(|(idx, r)| (idx, (r.sect, r.pt_idx)));

                    self.modal = Some(Modal::TrackList(TrackListState {
                        track_id: id,
                        rows,
                        selected_points,
                        last_clicked: first_selected.map(|(_, point)| point),
                    }));
                    self.tracks_cache.clear();

                    if let Some((row_idx, _)) = first_selected {
                        let step = crate::TRACK_LIST_ROW_HEIGHT + crate::TRACK_LIST_ROW_SPACING;
                        let y = (row_idx as f32 * step
                            + crate::TRACK_LIST_ROW_HEIGHT / 2.0
                            - crate::TRACK_LIST_VIEW_HEIGHT / 2.0).max(0.0);
                        return iced::widget::operation::scroll_to(
                            crate::TRACK_LIST_SCROLL_ID,
                            iced::widget::operation::AbsoluteOffset {
                                x: None,
                                y: Some(y),
                            },
                        );
                    }
                }
            }
            Message::CloseTrackList => {
                self.modal_drag_offset = Point::ORIGIN;
                self.modal = None;
                self.tracks_cache.clear();
            }
            Message::SelectTrackPoint(sect, pt_idx) => {
                if let Some(Modal::TrackList(state)) = &mut self.modal {
                    let ctrl = self.modifiers.control() || self.modifiers.command();
                    let shift = self.modifiers.shift();
                    let clicked = (sect, pt_idx);

                    if shift {
                        if let Some(last) = state.last_clicked {
                            if let (Some(start), Some(end)) = (
                                state.rows.iter().position(|r| r.sect == last.0 && r.pt_idx == last.1),
                                state.rows.iter().position(|r| r.sect == sect && r.pt_idx == pt_idx),
                            ) {
                                let (min, max) = if start < end { (start, end) } else { (end, start) };
                                for i in min..=max {
                                    state.selected_points.insert((state.rows[i].sect, state.rows[i].pt_idx));
                                }
                            }
                        } else {
                            state.selected_points.insert(clicked);
                        }
                    } else if ctrl {
                        if !state.selected_points.remove(&clicked) {
                            state.selected_points.insert(clicked);
                        }
                        state.last_clicked = Some(clicked);
                    } else {
                        state.selected_points.clear();
                        state.selected_points.insert(clicked);
                        state.last_clicked = Some(clicked);
                    }
                }
                self.sync_tracklist_selection();
                self.tracks_cache.clear();
            }
            Message::DeleteSelectedTrackPoints => {
                if let Some(Modal::TrackList(state)) = &self.modal {
                    let id = state.track_id;
                    let selected = state.selected_points.clone();
                    
                    self.push_undo(t!("msg_0066"));
                    if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == id) {
                        let t_m = Arc::make_mut(t_mut);
                        for (sect_idx, seg) in t_m.segments.iter_mut().enumerate() {
                            let mut new_pts = Vec::new();
                            for (pt_idx, pt) in seg.points.iter().enumerate() {
                                if !selected.contains(&(sect_idx + 1, pt_idx)) {
                                    new_pts.push(pt.clone());
                                }
                            }
                            seg.points = new_pts;
                        }
                        // Удаляем пустые сегменты
                        t_m.segments.retain(|s| !s.points.is_empty());
                        domain::recompute_track_stats(t_m);
                    }
                    
                    // Deleted rows shift indices; drop only this track's selection.
                    self.clear_track_point_selection(id);
                    // Переоткрываем таблицу, чтобы обновить PID и индексы
                    let _ = self.update_track_list(Message::OpenTrackList(id));
                    self.tracks_cache.clear();
                }
            }
            Message::CenterOnTrackPoint(sect, pt_idx) => {
                if let Some(Modal::TrackList(state)) = &self.modal {
                    if let Some(row) = state.rows.iter().find(|r| r.sect == sect && r.pt_idx == pt_idx) {
                        let (cx, cy) = merc_of(row.lon, row.lat);
                        self.cx = cx;
                        self.cy = cy;
                        self.canvas_cache.clear();
                        self.grid_cache.clear();
                        self.osm_canvas_cache.clear();
                        self.tracks_cache.clear();
                    }
                }
            }
            Message::SplitTrackAtPoint(sect, pt_idx) => {
                if let Some(Modal::TrackList(state)) = &self.modal {
                    let id = state.track_id;
                    self.push_undo(t!("msg_0053"));
                    
                    if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == id) {
                        let t_m = Arc::make_mut(t_mut);
                        
                        if sect > 0 && sect <= t_m.segments.len() {
                            let seg_idx = sect - 1; 
                            let seg = &mut t_m.segments[seg_idx];
                            
                            if pt_idx > 0 && pt_idx < seg.points.len() - 1 {
                                let right_half = seg.points.split_off(pt_idx + 1);
                                t_m.segments.insert(seg_idx + 1, domain::TrackSegment { points: right_half });
                            }
                        }
                        domain::recompute_track_stats(t_m);
                    }
                    self.clear_track_point_selection(id);
                    let _ = self.update_track_list(Message::OpenTrackList(id));
                    self.tracks_cache.clear();
                }
            }
            Message::StitchSegments => {
                    let Some(Modal::TrackList(state)) = &self.modal else {
                        return Task::none();
                    };
                let id = state.track_id;

                let mut points: Vec<(usize, usize)> = state.selected_points.iter().copied().collect();
                if points.len() != 2 {
                    self.status = t!("msg_0067").into();
                    return Task::none();
                }
                points.sort();
                let (sect_a, pt_a) = points[0];
                let (sect_b, pt_b) = points[1];

                if sect_b != sect_a + 1 {
                    self.status = t!("msg_0068").into();
                    return Task::none();
                }

                let seg_a_idx = sect_a - 1;
                let seg_b_idx = sect_b - 1;

                // Валидация + расстояние — до мутации, чтобы не держать заимствование
                let (dist, err) = {
                    let Some(t) = self.tracks.iter().find(|t| t.id == id) else { return  Task::none() };
                    if seg_a_idx >= t.segments.len() || seg_b_idx >= t.segments.len() {
                        (0.0, Some(t!("msg_0069")))
                    } else if pt_a + 1 != t.segments[seg_a_idx].points.len() {
                        (0.0, Some(t!("msg_0070")))
                    } else if pt_b != 0 {
                        (0.0, Some(t!("msg_0071")))
                    } else {
                        let last_a = &t.segments[seg_a_idx].points[pt_a];
                        let first_b = &t.segments[seg_b_idx].points[pt_b];
                        let d = domain::haversine_m(last_a.lat, last_a.lon, first_b.lat, first_b.lon);
                        (d, None)
                    }
                };
                if let Some(msg) = err {
                    self.status = msg.into();
                    return Task::none();
                }

                self.push_undo(t!("msg_0072"));

                // Допуск «это одна и та же точка» — 1.5 метра. Увеличить — если
                // на стыках бывают более заметные GPS-расхождения, которые
                // пользователь всё равно хочет схлопнуть.
                const DUP_TOLERANCE_M: f64 = 1.5;
                let dedup = dist < DUP_TOLERANCE_M;

                if let Some(t_mut) = self.tracks.iter_mut().find(|t| t.id == id) {
                    let t_m = Arc::make_mut(t_mut);
                    let mut seg_b_points = t_m.segments[seg_b_idx].points.clone();
                    if dedup {
                        seg_b_points.remove(0);
                    }
                    t_m.segments[seg_a_idx].points.extend(seg_b_points);
                    t_m.segments.remove(seg_b_idx);
                    domain::recompute_track_stats(t_m);
                }

                // Segment indices changed: discard invalid selections before rebuilding.
                self.clear_track_point_selection(id);
                // Переоткрыть TrackList, чтобы обновить индексы строк
                let _ = self.update_track_list(Message::OpenTrackList(id));
                self.tracks_cache.clear();

                self.status = if dedup {
                    format!("Сшито, дубликат удалён (Δ{:.1} м)", dist)
                } else {
                    format!("Сшито с новым ребром (Δ{:.1} м)", dist)
                };
            }
            Message::SelectPointsOutsideGrid => {
                if let Some(Modal::TrackList(state)) = &mut self.modal {
                    state.selected_points.clear();
                    if let Some(g) = &self.grid {
                        for r in &state.rows {
                            // Простейшая эвристика: если дальше 500 метров от ближайшего узла сетки
                            if let Some(n) = g.nearest(r.lat, r.lon, self.declination) {
                                if n.distance_m > 500.0 {
                                    state.selected_points.insert((r.sect, r.pt_idx));
                                }
                            }
                        }
                    }
                }
                self.sync_tracklist_selection();
                self.tracks_cache.clear();
            }
            Message::TrackListUp | Message::TrackListDown => {
                if let Some(Modal::TrackList(state)) = &mut self.modal {
                    if state.rows.is_empty() { return Task::none(); }

                    // Находим текущий индекс выделенной точки
                    let current_idx = state.last_clicked.and_then(|(s, p)| {
                        state.rows.iter().position(|r| r.sect == s && r.pt_idx == p)
                    });

                    // Если ничего не выделено — ставим на первую
                    let next_idx = match current_idx {
                        None => 0,
                        Some(idx) => {
                            if matches!(m, Message::TrackListUp) {
                                idx.saturating_sub(1)
                            } else {
                                (idx + 1).min(state.rows.len() - 1)
                            }
                        }
                    };

                    let row = &state.rows[next_idx];
                    let new_clicked = (row.sect, row.pt_idx);
                    
                    // Если зажат Shift, добавляем к выделению, иначе сбрасываем
                    let shift = self.modifiers.shift();
                    if !shift {
                        state.selected_points.clear();
                    }
                    state.selected_points.insert(new_clicked);
                    state.last_clicked = Some(new_clicked);
                }
                self.sync_tracklist_selection();
                self.tracks_cache.clear();
            }
            Message::DeleteTrackById(id) => {
                self.push_undo(t!("msg_0052"));
                self.tracks.retain(|t| t.id != id);
                self.selection.tracks.remove(&id);
                self.tracks_cache.clear();
            }
            Message::FitTrackById(id) => {
                self.selection.tracks.clear();
                self.selection.tracks.insert(id);
                self.update_tracks(Message::FitTrackToScreen);
            }
            _ => {}
        }
        Task::none()
    }
    
    // =========================================================================
    // Восстановление выделения трека на карте
    // =========================================================================
    
    fn hit_test_track_at(&self, p: Point, threshold: f32) -> Option<u64> {
        let sz = self.last_bounds.get();
        let hw = sz.width as f64 / 2.0;
        let hh = sz.height as f64 / 2.0;

        let mut best: Option<(f32, u64)> = None;

        for t in &self.tracks {
            if !t.visible {
                continue;
            }
            for seg in &t.segments {
                if seg.points.len() < 2 {
                    continue;
                }
                for w in seg.points.windows(2) {
                    let (mx1, my1) = merc_of(w[0].lon, w[0].lat);
                    let (mx2, my2) = merc_of(w[1].lon, w[1].lat);
                    let p1 = Point::new(
                        (hw + (mx1 - self.cx) / self.scale) as f32,
                        (hh - (my1 - self.cy) / self.scale) as f32,
                    );
                    let p2 = Point::new(
                        (hw + (mx2 - self.cx) / self.scale) as f32,
                        (hh - (my2 - self.cy) / self.scale) as f32,
                    );
                    let d = dist_to_segment(p, p1, p2);
                    if d < threshold && best.map_or(true, |(bd, _)| d < bd) {
                        best = Some((d, t.id));
                    }
                }
            }
        }
        best.map(|(_, id)| id)
    }

    // =========================================================================
    // 9. Вспомогательные методы
    // =========================================================================
    pub(crate) fn update_track_preview(&mut self, id: u64) {
        if let Some(Modal::TrackProps { draft, .. }) = &self.modal {
            if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                let t_mut = Arc::make_mut(t);
                t_mut.props = draft.clone();
                t_mut.name = draft.description.clone();
                t_mut.color = draft.line_color;
                self.tracks_cache.clear();
            }
        }
    }

    /// Пересчитывает магнитное склонение по центру карты не чаще раза в 10 сек.
    /// `force = true` — вычислить немедленно (после fit, конца панорамирования,
    /// загрузки комплекта).
    pub(crate) fn maybe_update_declination(&mut self, force: bool) {
        const THROTTLE: Duration = Duration::from_secs(10);
        let now = Instant::now();
        let should_calc = force
            || self.last_declination_calc
                .map_or(true, |t| now.duration_since(t) >= THROTTLE);
        if !should_calc {
            return;
        }
        let (y, ord) = crate::util::coords::today();
        let (lon, lat) = lonlat_of(self.cx, self.cy);
        if let Ok(d) = declination::declination_deg(lat, lon, y, ord) {
            self.declination = d;
        }
        self.last_declination_calc = Some(now);
    }

    pub(crate) fn update_waypoint_preview(&mut self, id: u64) {
        if let Some(Modal::WaypointProps { draft, .. }) = &self.modal {
            if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == id) {
                *w = Arc::new(draft.clone());
                self.tracks_cache.clear();
            }
        }
    }
    
    fn save_dialog_with_dir(
        &self,
        default_name: &str,
        filter_name: &str,
        filter_ext: &[&str],
    ) -> rfd::AsyncFileDialog {
        let mut dialog = rfd::AsyncFileDialog::new()
            .set_file_name(default_name)
            .add_filter(filter_name, filter_ext);
        if let Some(dir) = &self.last_save_dir {
            dialog = dialog.set_directory(dir);
        } else if let Some(kit) = &self.last_kit {
            let sub = kit.join("10-Tracks");
            if sub.is_dir() {
                dialog = dialog.set_directory(&sub);
            } else {
                dialog = dialog.set_directory(kit);
            }
        }
        dialog
    }

    
    fn fit_to_grid(&mut self) -> bool {
        if self.grid.is_none() {
            return false;
        }
        
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut found = false;
        
        for line in &self.lines {
            for &(mx, my) in line {
                if !mx.is_finite() || !my.is_finite() {
                    continue;
                }
                if mx < min_x { min_x = mx; }
                if mx > max_x { max_x = mx; }
                if my < min_y { min_y = my; }
                if my > max_y { max_y = my; }
                found = true;
            }
        }
        
        if !found {
            return false;
        }
        
        let w = (max_x - min_x).max(0.001);
        let h = (max_y - min_y).max(0.001);
        let sz = self.last_bounds.get();
        let vw = sz.width as f64;
        let vh = sz.height as f64;
        
        let (pad_x, pad_y) = if vw < vh { (100.0, 0.0) } else { (0.0, 100.0) };
        let scale_x = w / (vw - pad_x).max(1.0);
        let scale_y = h / (vh - pad_y).max(1.0);
        
        self.scale = scale_x.max(scale_y).max(0.1).min(10000.0);
        self.cx = (min_x + max_x) / 2.0;
        self.cy = (min_y + max_y) / 2.0;
        
        true
    }
    
}

