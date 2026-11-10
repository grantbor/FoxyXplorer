use std::time::Instant;
use iced::mouse::ScrollDelta;
use iced::{Point, Task};
use coord::CoordFormat;

use crate::{App, Message, Modal};
use crate::domain;
use crate::io;
use crate::session;
use crate::util::coords::{build_template, date_offset, parse_coords};
use crate::util::geo::lonlat_of;
use crate::{load_kit, save_session};

use std::sync::Arc;
use crate::UndoState;

use std::collections::HashSet; 
use crate::{TrackRowData, TrackListState}; 
use crate::util::geo::merc_of; 

impl App {
    pub(crate) fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            // --- View & Canvas ---
            Message::Pressed | Message::Released | Message::Moved(_) | Message::Scrolled(_)
            | Message::Fit | Message::WindowResized(_) | Message::WindowMoved(_)
            | Message::DividerPressed | Message::DividerReleased | Message::CursorMoved(_)  => {
                self.update_view(m);
            }

            // --- UI (Верхняя панель) ---
            Message::ToggleGrid | Message::ToggleLabels | Message::CycleFormat 
            | Message::SearchTracksChanged(_) | Message::SearchWaypointsChanged(_) | Message::Undo | Message::Redo 
            | Message::ModalDragStart | Message::ModalDragEnd=> {
                self.update_ui(m); 
            }

            // --- Треки ---
            Message::Layer(_) | Message::SelectTrack(_) | Message::ToggleTrackVisible(_, _)
            | Message::ToggleAllTracksVisible | Message::DeleteSelectedTracks
            | Message::OpenTrackProps(_) | Message::ApplyTrackProps | Message::CloseTrackProps
            | Message::ToggleTrackSaveMenu | Message::SimplifyTracks |  Message::FitTrackToScreen=> {
                self.update_tracks(m);
            }

            // --- Модальное окно трека ---
            Message::DraftDescChanged(_) | Message::DraftDateChanged(_) | Message::DraftCallsignChanged(_)
            | Message::DraftNum1Changed(_) | Message::DraftNum2Changed(_) | Message::DraftSuffixChanged(_)
            | Message::DraftDateToday | Message::DraftDateYesterday 
            | Message::DraftColorChanged(_) | Message::DraftFillColorChanged(_) | Message::DraftWidthChanged(_)
            | Message::DraftStyleChanged(_) | Message::DraftTypeChanged(_) | Message::DraftFillTypeChanged(_)
            | Message::ToggleColorPicker => {
                self.update_track_modal(m);
            }

            // --- Точки ---
            Message::SelectWaypoint(_) | Message::ToggleWaypointVisible(_, _)
            | Message::ToggleAllWaypointsVisible | Message::DeleteSelectedWaypoints
            | Message::OpenWaypointProps(_) | Message::AddNewWaypoint | Message::ApplyWaypointProps
            | Message::CloseWaypointProps | Message::ToggleWpSaveMenu | Message::CenterOnWaypoint(_)
            | Message::ClearWaypointCrosshair => {
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
            | Message::SaveWaypointsWptNew | Message::SaveWaypointsWptNewPicked(_)
            | Message::SaveWaypointsWptAppend | Message::SaveWaypointsWptAppendPicked(_)
            | Message::SaveTrackProps | Message::SaveTrackPropsPicked(_) => {
                return self.update_save(m);
            }

            // --- Система и прочее ---
            Message::PickKit | Message::KitPicked(_) | Message::PickTrack | Message::TracksPicked(_)
            | Message::WindowCloseRequested | Message::ModifiersChanged(_) | Message::OsmTileLoaded(_, _)
            | Message::RefreshSystemTheme | Message::None => {
                return self.update_system(m);
            }
            
            // --- Tracklist ----
            Message::OpenTrackList(_) | Message::CloseTrackList | Message::SelectTrackPoint(_, _)
            | Message::SelectAllTrackPoints | Message::DeleteSelectedTrackPoints
            | Message::CenterOnTrackPoint(_, _) | Message::SplitTrackAtPoint(_, _) | Message::TrackListUp | Message::TrackListDown
            | Message::SelectPointsOutsideGrid => {
                self.update_track_list(m);
            }
        }

        self.request_osm_tiles()
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

    // =========================================================================
    // 1. Логика View (Панорамирование, зум, ресайз окна)
    // =========================================================================
    fn update_view(&mut self, m: Message) {
        match m {
            Message::Pressed => self.dragging = true,
            Message::Released => {
                if self.dragging {
                    self.dragging = false;
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.refresh_status();
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
                } else {
                    self.cursor = Some(p);
                    self.refresh_status();
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
                self.refresh_status();
            }
            Message::Fit => {
                if !self.osm_active() {
                    let idx = self.active;
                    self.fit_to_layer(idx);
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
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
    fn update_ui(&mut self, m: Message) {
        match m {
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
                self.tracks_cache.clear();
            }
            Message::SearchWaypointsChanged(q) => {
                self.search_waypoints = q;
                self.tracks_cache.clear();
            }
            Message::Undo => {
                if let Some(prev) = self.undo_stack.pop() {
                    self.redo_stack.push(UndoState {
                        tracks: self.tracks.clone(),
                        waypoints: self.waypoints.clone(),
                        next_id: self.next_id,
                        label: prev.label.clone(),
                    });
                    self.tracks = prev.tracks;       // Оба типа теперь Vec<Arc<Track>>
                    self.waypoints = prev.waypoints; // Оба типа теперь Vec<Arc<Waypoint>>
                    self.next_id = prev.next_id;
                    self.tracks_cache.clear();
                    self.status = format!("Отменено: {}", prev.label);
                }
            }
            Message::Redo => {
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
            Message::ModalDragStart => {
                if self.modal.is_some() {
                    self.modal_dragging = true;
                    self.modal_drag_start_cursor = None;
                    self.modal_drag_start_offset = self.modal_drag_offset;
                }
            }
            Message::ModalDragEnd => {
                self.modal_dragging = false;
            }
            _ => {}
        }
    }

    // =========================================================================
    // 3. Треки (Логика списка)
    // =========================================================================
    fn update_tracks(&mut self, m: Message) {
        match m {
            Message::Layer(i) => {
                if i <= self.layers.len() {
                    self.active = i;
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                    self.refresh_status();
                }
            }
            Message::SelectTrack(id) => {
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
                if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                    Arc::make_mut(t).visible = v; 
                    self.tracks_cache.clear();
                }
            }
            Message::ToggleAllTracksVisible => {
                let all_visible = self.tracks.iter().all(|t| t.visible);
                for t in &mut self.tracks {
                    Arc::make_mut(t).visible = !all_visible; 
                }
                self.tracks_cache.clear();
            }
             Message::DeleteSelectedTracks => {
                self.push_undo("Удалить треки"); 
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
                    self.push_undo("Изменить свойства трека");
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
                self.modal = None;
            }
            Message::ToggleTrackSaveMenu => {
                self.show_track_save_menu = !self.show_track_save_menu;
                self.show_wp_save_menu = false;
            }
            Message::SimplifyTracks => {
                let ids = self.selection.tracks.clone();
                self.push_undo("Упростить трек"); // <-- ДОБАВЛЕНО
                for id in ids {
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) {
                        domain::simplify::simplify_track(Arc::make_mut(t), self.simplify_tolerance);
                    }
                }
                self.tracks_cache.clear();
                self.status = format!("Треки упрощены (допуск {} м)", self.simplify_tolerance);
            }
                        Message::FitTrackToScreen => {
                let ids = self.selection.tracks.clone();
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
                                min_x = min_x.min(mx); max_x = max_x.max(mx);
                                min_y = min_y.min(my); max_y = max_y.max(my);
                                found = true;
                            }
                        }
                    }
                }

                if found {
                    let w = max_x - min_x;
                    let h = max_y - min_y;
                    let sz = self.last_bounds.get();
                    
                    let pad_x = if sz.width < sz.height { 100.0 } else { 0.0 };
                    let pad_y = if sz.height <= sz.width { 100.0 } else { 0.0 };

                    let scale_x = if w > 0.0 { w / (sz.width as f64 - pad_x as f64) } else { 0.0 };
                    let scale_y = if h > 0.0 { h / (sz.height as f64 - pad_y as f64) } else { 0.0 };
                    
                    self.scale = scale_x.max(scale_y).max(0.1);
                    self.cx = (min_x + max_x) / 2.0;
                    self.cy = (min_y + max_y) / 2.0;
                    
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.osm_canvas_cache.clear();
                    self.tracks_cache.clear();
                }
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
                    let is_lisa_like = matches!(draft.callsign.as_str(), "Lisa" | "Avtonom" | "Kinolog" | "Pegas");
                    let forbidden = [[0, 255, 255], [255, 0, 255], [255, 255, 255], [0, 0, 0]];
                    if is_lisa_like && forbidden.contains(&c) {
                        draft.line_color = [255, 0, 0];
                    } else {
                        draft.line_color = c;
                    }
                    tid = Some(*track_id);
                }
                self.show_color_picker = false;
                if let Some(id) = tid { self.update_track_preview(id); }
            }
            Message::DraftFillColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::TrackProps { track_id, draft }) = &mut self.modal { draft.fill_color = c; tid = Some(*track_id); }
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
            Message::ToggleColorPicker => {
                self.show_color_picker = !self.show_color_picker;
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
                if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == id) {
                    Arc::make_mut(w).visible = v; 
                    self.tracks_cache.clear();
                }
            }
            Message::ToggleAllWaypointsVisible => {
                let all_visible = self.waypoints.iter().all(|w| w.visible);
                for w in &mut self.waypoints {
                    Arc::make_mut(w).visible = !all_visible; 
                }
                self.tracks_cache.clear();
            }
            Message::DeleteSelectedWaypoints => {
                self.push_undo("Удалить точки"); 
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
                    self.push_undo("Изменить свойства точки");
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
                self.push_undo("Добавить точку");
                let (lon, lat) = lonlat_of(self.cx, self.cy);
                let new_wp = domain::Waypoint {
                    id: self.next_id,
                    name: "New WP".into(),
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

                    // ОБЯЗАТЕЛЬНО return, иначе таймер не запустится!
                    return Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        },
                        |_| Message::ClearWaypointCrosshair,
                    );
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
            Message::DraftWpTextColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.text_color = c; tid = Some(*waypoint_id);
                }
                if let Some(id) = tid { self.update_waypoint_preview(id); }
            }
            Message::DraftWpBgColorChanged(c) => {
                let mut tid = None;
                if let Some(Modal::WaypointProps { waypoint_id, draft, .. }) = &mut self.modal {
                    draft.bg_color = c; tid = Some(*waypoint_id);
                }
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

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{}.plt", name))
                    .add_filter("OziExplorer Track", &["plt"]);
                
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksPltPicked);
            }
            Message::SaveTracksPltPicked(Some(path)) => {
                let sel_tracks: Vec<_> = self.tracks.iter().filter(|t| self.selection.tracks.contains(&t.id)).map(|t| t.as_ref()).collect();
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).map(|w| w.as_ref()).collect();
                if let Err(e) = io::save_any(&sel_tracks, &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
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

                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("{}.ms", name))
                    .add_filter("Guru Maps", &["ms"]);
                
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveTracksMsPicked);
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
                    Message::SaveWaypointsWptNewPicked
                );
            }
            Message::SaveWaypointsWptNewPicked(Some(path)) => {
                let sel_wps: Vec<_> = self.waypoints.iter().filter(|w| self.selection.waypoints.contains(&w.id)).map(|w| w.as_ref()).collect();
                if let Err(e) = io::save_any(&[], &sel_wps, &path) {
                    self.status = format!("Ошибка: {e}");
                } else {
                    self.status = format!("Сохранено в {}", path.display());
                }
            }
            Message::SaveWaypointsWptNewPicked(None) => {}

            Message::SaveWaypointsWptAppend => {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .add_filter("OziExplorer Waypoints", &["wpt"]);
                if let Some(kit) = &self.last_kit {
                    let dir = kit.join("10-Tracks");
                    if dir.is_dir() { dialog = dialog.set_directory(&dir); } else { dialog = dialog.set_directory(kit); }
                }
                return Task::perform(async move { dialog.pick_file().await.map(|h| h.path().to_path_buf()) }, Message::SaveWaypointsWptAppendPicked);
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
                // 1. Достаем id, чтобы освободить self.modal
                let track_id = if let Some(Modal::TrackProps { track_id, .. }) = &self.modal {
                    *track_id
                } else {
                    return Task::none(); // или можно просто ничего не делать
                };

                // 2. Сохраняем на диск (только читаем данные)
                let save_result = if let Some(t) = self.tracks.iter().find(|t| t.id == track_id) {
                    io::save_plt(t, &path)
                } else {
                    Err("Трек не найден".to_string())
                };

                // 3. Обрабатываем результат и меняем self
                match save_result {
                    Err(e) => {
                        self.status = format!("Ошибка сохранения: {e}");
                    }
                    Ok(_) => {
                        self.push_undo("Сохранение трека в файл");
                        self.status = format!("Сохранено в {}", path.display());
                        
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
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Track/Points", &["gpx", "plt", "ms", "wpt"])
                            .pick_files()
                            .await
                    },
                    |files| Message::TracksPicked(
                        files.map(|fs| fs.into_iter().map(|f| f.path().to_path_buf()).collect())
                    ),
                );
            }
            Message::TracksPicked(Some(paths)) => {
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
            Message::OsmTileLoaded(id, result) => {
                self.osm_cache.unmark_pending(id);
                let handle = result
                    .ok()
                    .map(|bytes| iced::widget::image::Handle::from_bytes(bytes));
                self.osm_cache.insert(id, handle);
                self.osm_canvas_cache.clear();
            }
            Message::RefreshSystemTheme => {
                if let Ok((theme, resolved, _, _)) = native_theme_iced::from_system() {
                    self.theme = theme;
                    self.resolved_theme = resolved;
                }
            }
            Message::None => {}
            _ => {}
        }
        Task::none()
    }
    
    fn update_track_list(&mut self, m: Message) {
        match m {
            Message::OpenTrackList(id) => {
                self.modal_drag_offset = Point::ORIGIN;
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

                    self.modal = Some(Modal::TrackList(TrackListState {
                        track_id: id,
                        rows,
                        selected_points: HashSet::new(),
                        last_clicked: None,
                    }));
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
                self.tracks_cache.clear();
            }
            Message::SelectAllTrackPoints => {
                if let Some(Modal::TrackList(state)) = &mut self.modal {
                    state.selected_points.clear();
                    for r in &state.rows {
                        state.selected_points.insert((r.sect, r.pt_idx));
                    }
                }
                self.tracks_cache.clear();
            }
            Message::DeleteSelectedTrackPoints => {
                self.modal_drag_offset = Point::ORIGIN;
                if let Some(Modal::TrackList(state)) = &self.modal {
                    let id = state.track_id;
                    let selected = state.selected_points.clone();
                    
                    self.push_undo("Удалить точки трека");
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
                    
                    // Переоткрываем таблицу, чтобы обновить PID и индексы
                    self.modal = None;
                    self.update_track_list(Message::OpenTrackList(id));
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
                self.modal_drag_offset = Point::ORIGIN;
                if let Some(Modal::TrackList(state)) = &self.modal {
                    let id = state.track_id;
                    self.push_undo("Разделить трек");
                    
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
                    self.modal = None;
                    self.update_track_list(Message::OpenTrackList(id));
                    self.tracks_cache.clear();
                }
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
            }
            Message::TrackListUp | Message::TrackListDown => {
                if let Some(Modal::TrackList(state)) = &mut self.modal {
                    if state.rows.is_empty() { return; }

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
                self.tracks_cache.clear();
            }
            _ => {}
        }
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

    pub(crate) fn update_waypoint_preview(&mut self, id: u64) {
        if let Some(Modal::WaypointProps { draft, .. }) = &self.modal {
            if let Some(w) = self.waypoints.iter_mut().find(|w| w.id == id) {
                *w = Arc::new(draft.clone());
                self.tracks_cache.clear();
            }
        }
    }
}
