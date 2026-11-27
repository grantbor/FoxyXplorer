//! Track review lives in the sidebar so that the map stays unobstructed.
use iced::widget::{checkbox, column, container, pick_list, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use crate::{App, Message};
use crate::domain::recovery::Diagnosis;
use crate::domain::quality::{Verdict, ReviewReason};

// Keep short jumps readable: printing a 40 m displacement as 0.0 km
// hides precisely the errors this mode is intended to detect.
fn format_jump_distance(m: f64) -> String {
    if m < 1_000.0 {
        format!("{:.0} {}", m, crate::t!("msg_0479"))
    } else {
        format!("{:.2} {}", m / 1_000.0, crate::t!("msg_0480"))
    }
}

/// Original point numbers remain stable across preview cuts and GPX segments.
fn original_number_label(
    map: &[Vec<(usize, usize)>], original: &crate::domain::Track,
    segment: usize, start: usize, end: usize,
) -> Option<String> {
    let points = map.get(segment)?;
    if start > end { return None; }
    let &(a_seg, a_idx) = points.get(start)?;
    let &(b_seg, b_idx) = points.get(end)?;
    let global = |seg: usize, idx: usize| -> usize {
        original.segments.iter().take(seg).map(|s| s.points.len()).sum::<usize>() + idx + 1
    };
    Some(crate::tf!("msg_0520", first=global(a_seg, a_idx), last=global(b_seg, b_idx)))
}

impl App {
    pub(crate) fn view_recovery_panel(&self) -> Element<'_, Message> {
        let inspect = self.track_inspection.as_ref().expect("inspection panel state");
        let report = &inspect.report;
        let track_name = self.tracks.iter().find(|t| t.id == inspect.track_id)
            .map(|t| t.name.as_str()).unwrap_or("-");
        let diagnosis = match report.diagnosis {
            Diagnosis::Chronology => crate::t!("msg_0437"),
            Diagnosis::SuspectCoordinates => crate::t!("msg_0438"),
            Diagnosis::InsufficientData => crate::t!("msg_0439"),
            Diagnosis::NoObviousAnomalies => crate::t!("msg_0440"),
        };
        let mut content = column![
            text(track_name).size(13),
            text(diagnosis).size(16),
            text(crate::tf!("msg_0444", count = report.points)).size(12),
            text(crate::tf!("msg_0443", count = report.reversals)).size(12),
            text(crate::tf!("msg_0441", distance = format!("{:.2}", report.original_distance_m / 1_000.0))).size(12),
        ].spacing(9).width(Length::Fill);

        content = content.push(text(crate::t!("msg_0466")).size(12));
        content = content.push(
            pick_list(
                [crate::domain::recovery::MovementMode::Unknown,
                 crate::domain::recovery::MovementMode::Foot,
                 crate::domain::recovery::MovementMode::Vehicle],
                Some(inspect.movement_mode),
                Message::InspectionMovementChanged,
            ).width(Length::Fill)
        );
        content = content.push(text(crate::t!("msg_0477")).size(11));
        content = content.push(text(crate::tf!("msg_0473", count = report.missing_times)).size(12));
        content = content.push(text(crate::tf!("msg_0474", count = report.repeated_times)).size(12));
        content = content.push(text(crate::tf!("msg_0475", distance = format_jump_distance(report.original_max_step_m))).size(12));

        content = content.push(text(crate::tf!("msg_0487", count = report.quality.hdop_samples)).size(12));
        content = content.push(text(crate::tf!("msg_0488", count = report.quality.speed_samples)).size(12));
        content = content.push(text(crate::tf!("msg_0489", count = report.quality.poor_quality_samples)).size(12));

        if report.can_reorder {
            content = content.push(text(crate::tf!("msg_0442", distance = format!("{:.2}", report.reordered_distance_m / 1_000.0))).size(12));
            content = content.push(text(crate::tf!("msg_0476", distance = format_jump_distance(report.reordered_max_step_m))).size(12));
            content = content.push(text(crate::t!("msg_0461")).size(12));
        } else {
            if report.extensive_damage {
                content = content.push(text(crate::t!("msg_0483")).size(12));
            }
            content = content.push(text(crate::tf!("msg_0445", count = report.severe_transitions)).size(12));
            if report.severe_transitions > 0 {
                content = content.push(text(crate::t!("msg_0465")).size(11));
            }
            content = content.push(text(crate::tf!("msg_0446", count = report.ranges.len())).size(12));
            let unresolved_count = if inspect.selected.is_empty() && inspect.selected_reviews.is_empty() {
                report.unresolved_jump_indices().len()
            } else if inspect.selected_reviews.is_empty() {
                report.unresolved_jump_indices_for(inspect.selected.iter().copied()).len()
            } else {
                report.unresolved_jump_indices_with_reviews(inspect.selected.iter().copied(), inspect.selected_reviews.iter().copied()).len()
            };
            content = content.push(text(crate::tf!("msg_0490", count = unresolved_count)).size(12));
            if !report.ranges.is_empty() {
                content = content.push(text(crate::t!("msg_0447")).size(12));
                content = content.push(text(crate::t!("msg_0459")).size(12));
                let mut issues = column![].spacing(7);
                for (i, range) in report.ranges.iter().enumerate() {
                    let label = crate::tf!("msg_0448",
                        segment = range.segment + 1,
                        start = range.start + 1,
                        end = range.end + 1,
                        count = range.count());
                    let info = crate::tf!("msg_0449",
                        entry = format_jump_distance(range.entry_m),
                        exit = format_jump_distance(range.exit_m),
                        bridge = format_jump_distance(range.bridge_m));
                    let focused = inspect.focused == Some(i);
                    issues = issues.push(
                        container(column![
                            checkbox(inspect.selected.contains(&i))
                                .label(label)
                                .on_toggle(move |checked| Message::InspectionToggle(i, checked))
                                .size(14),
                            text(if report.quality.episodes.iter().any(|ep| ep.verdict == Verdict::Suggest
                                && ep.segment == range.segment && ep.start == range.start && ep.end == range.end) {
                                crate::t!("msg_0492")
                            } else if range.count() == 1 {
                                crate::t!("msg_0470")
                            } else { crate::t!("msg_0481") }).size(11),
                            text(info).size(11),
                            self.btn_secondary(crate::t!("msg_0450"))
                                .on_press(Message::InspectionFocus(i)),
                        ].spacing(4))
                        .padding(7)
                        .width(Length::Fill)
                        .style(move |theme: &iced::Theme| {
                            let palette = theme.extended_palette();
                            container::Style {
                                background: if focused { Some(palette.primary.weak.color.into()) }
                                    else { Some(palette.background.weak.color.into()) },
                                ..Default::default()
                            }
                        }),
                    );
                }
                content = content.push(row![
                    self.btn_secondary(crate::t!("msg_0451")).on_press(Message::InspectionSelectAll(true)),
                    self.btn_secondary(crate::t!("msg_0452")).on_press(Message::InspectionSelectAll(false)),
                ].spacing(5));
                content = content.push(issues);
            }
            let mut quality_reviews = column![].spacing(7);
            for (i, ep) in report.quality.episodes.iter().enumerate() {
                let label = crate::tf!("msg_0491", segment = ep.segment + 1,
                    start = ep.start + 1, end = ep.end + 1, count = ep.count());
                let bridge = if ep.bridge_m.is_finite() {
                    format_jump_distance(ep.bridge_m)
                } else { "—".to_string() };
                let details = crate::tf!("msg_0494", bad = ep.bad_fixes,
                    jumps = ep.suspect_steps, distance = format_jump_distance(ep.internal_m),
                    bridge = bridge);
                if ep.verdict == Verdict::Review {
                    let mut review = column![
                        checkbox(inspect.selected_reviews.contains(&i))
                            .label(label).on_toggle(move |checked| Message::InspectionToggleReview(i, checked))
                            .size(14),
                        text(details).size(11),
                    ].spacing(3);
                    if let Some(reason) = ep.reason {
                        let explanation = match reason {
                            ReviewReason::SegmentEdge => crate::t!("msg_0505"),
                            ReviewReason::TrackTail => crate::t!("msg_0506"),
                            ReviewReason::QualitySupportedHook => crate::t!("msg_0507"),
                            ReviewReason::LateralExcursion => crate::t!("msg_0508"),
                        };
                        review = review.push(text(explanation).size(11));
                    }
                    quality_reviews = quality_reviews.push(review.push(
                        self.btn_secondary(crate::t!("msg_0450"))
                            .on_press(Message::InspectionFocusQuality(i)),
                    ));
                }
            }
            if report.quality.episodes.iter().any(|ep| ep.verdict == Verdict::Review) {
                content = content.push(text(crate::t!("msg_0493")).size(13));
                content = content.push(text(crate::t!("msg_0498")).size(11));
                content = content.push(row![
                    self.btn_secondary(crate::t!("msg_0451"))
                        .on_press(Message::InspectionSelectAllReviews(true)),
                    self.btn_secondary(crate::t!("msg_0452"))
                        .on_press(Message::InspectionSelectAllReviews(false)),
                ].spacing(5));
                content = content.push(quality_reviews);
            }
            if report.quality.hdop_samples == 0 && report.quality.speed_samples == 0 {
                content = content.push(text(crate::t!("msg_0495")).size(11));
            }
            // Every transition not accounted for by a proposed range must
            // remain visible, even if the algorithm cannot identify which
            // neighbour is erroneous. These rows do not have delete controls.
            let unresolved = report.unresolved_jump_indices_with_reviews(inspect.selected.iter().copied(),inspect.selected_reviews.iter().copied());
            if !unresolved.is_empty() {
                content = content.push(text(crate::t!("msg_0482")).size(13));
                let mut jumps = column![].spacing(5);
                for index in unresolved.into_iter().take(40) {
                    let jump = &report.severe_jumps[index];
                    let label = crate::tf!("msg_0472",
                        segment = jump.segment + 1,
                        start = jump.to_index,
                        end = jump.to_index + 1,
                        distance = format_jump_distance(jump.distance_m),
                        speed = format!("{:.1}", jump.speed_m_s * 3.6));
                    jumps = jumps.push(column![
                        text(label).size(11),
                        self.btn_secondary(crate::t!("msg_0450"))
                            .on_press(Message::InspectionFocusJump(index)),
                    ].spacing(3));
                }
                content = content.push(jumps);
            }
            if matches!(report.diagnosis, Diagnosis::InsufficientData) {
                content = content.push(text(crate::t!("msg_0462")).size(12));
            } else if matches!(report.diagnosis, Diagnosis::NoObviousAnomalies) {
                content = content.push(text(crate::t!("msg_0463")).size(12));
            }
        }

        content = content.push(text(crate::t!("msg_0499")).size(11));
        content = content.push(text(crate::t!("msg_0460")).size(11));
        content = content.push(text(crate::t!("msg_0458")).size(11));
        content = content.push(text(crate::t!("msg_0464")).size(11));

        let original_for_numbers = self.tracks.iter().find(|t| t.id == inspect.track_id);
        let second_number_map = original_for_numbers
            .map(|original| crate::app::recovery::first_pass_source_indices(inspect, original));

        // The first preview already contains a fresh diagnosis of the derived
        // segments. Unlike v3.4, expose those findings for a second approved
        // cut in the SAME transaction. Indexes are relative to the preview.
        if inspect.preview_segments.is_some() {
            if let Some(second) = &inspect.second_report {
                content = content.push(text(crate::t!("msg_0510")).size(14));
                content = content.push(text(crate::t!("msg_0511")).size(11));
                if second.ranges.is_empty() && !second.quality.episodes.iter()
                    .any(|ep| ep.verdict == Verdict::Review) {
                    content = content.push(text(crate::t!("msg_0515")).size(11));
                }
                if !second.ranges.is_empty() {
                    content = content.push(text(crate::t!("msg_0512")).size(12));
                    let mut second_items = column![].spacing(6);
                    for (index, range) in second.ranges.iter().enumerate() {
                        let label = crate::tf!("msg_0448",
                            segment=range.segment+1, start=range.start+1,
                            end=range.end+1, count=range.count());
                        let focused = inspect.second_focused == Some((false,index));
                        second_items = second_items.push(container(column![
                            checkbox(inspect.second_selected.contains(&index))
                                .label(label)
                                .on_toggle(move |checked| Message::InspectionToggleSecond(index, checked))
                                .size(14),
                            text(original_for_numbers.and_then(|original| second_number_map.as_ref()
                                .and_then(|map| original_number_label(map, original,
                                    range.segment, range.start, range.end)))
                                .unwrap_or_default()).size(11),
                            text(crate::tf!("msg_0449",
                                entry=format_jump_distance(range.entry_m),
                                exit=format_jump_distance(range.exit_m),
                                bridge=format_jump_distance(range.bridge_m))).size(11),
                            self.btn_secondary(crate::t!("msg_0450"))
                                .on_press(Message::InspectionFocusSecond(index, false)),
                        ].spacing(4)).padding(6).width(Length::Fill)
                            .style(move |theme: &iced::Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(if focused { palette.primary.weak.color }
                                        else { palette.background.weak.color }.into()),
                                    ..Default::default()
                                }
                            }));
                    }
                    content = content.push(second_items);
                }
                if second.quality.episodes.iter().any(|ep| ep.verdict == Verdict::Review) {
                    content = content.push(text(crate::t!("msg_0513")).size(12));
                    let mut second_reviews = column![].spacing(6);
                    for (index, ep) in second.quality.episodes.iter().enumerate() {
                        if ep.verdict != Verdict::Review { continue; }
                        let label = crate::tf!("msg_0491", segment=ep.segment+1,
                            start=ep.start+1, end=ep.end+1, count=ep.count());
                        let focused = inspect.second_focused == Some((true,index));
                        second_reviews = second_reviews.push(container(column![
                            checkbox(inspect.second_selected_reviews.contains(&index))
                                .label(label)
                                .on_toggle(move |checked| Message::InspectionToggleSecondReview(index, checked))
                                .size(14),
                            text(original_for_numbers.and_then(|original| second_number_map.as_ref()
                                .and_then(|map| original_number_label(map, original,
                                    ep.segment, ep.start, ep.end)))
                                .unwrap_or_default()).size(11),
                            text(crate::tf!("msg_0494",
                                bad=ep.bad_fixes, jumps=ep.suspect_steps,
                                distance=format_jump_distance(ep.internal_m),
                                bridge=if ep.bridge_m.is_finite() {
                                    format_jump_distance(ep.bridge_m)
                                } else { "—".to_string() })).size(11),
                            self.btn_secondary(crate::t!("msg_0450"))
                                .on_press(Message::InspectionFocusSecond(index, true)),
                        ].spacing(4)).padding(6).width(Length::Fill)
                            .style(move |theme: &iced::Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(if focused { palette.primary.weak.color }
                                        else { palette.background.weak.color }.into()),
                                    ..Default::default()
                                }
                            }));
                    }
                    content = content.push(second_reviews);
                }
                content = content.push(text(crate::t!("msg_0514")).size(11));
            }
        }

        // An additional diagnosis can be requested only AFTER the operator
        // approves some findings of the latest pass. Every new pass is
        // indexed into its own preview snapshot, not into the source track.
        if inspect.preview_segments.is_some() {
            for (pass_index, pass) in inspect.extra_passes.iter().enumerate() {
                let extra_number_map = original_for_numbers.and_then(|original|
                    crate::app::recovery::source_indices_for_extra(inspect, original, pass_index));
                content = content.push(text(crate::tf!("msg_0516",
                    pass=pass_index + 3)).size(14));
                content = content.push(text(crate::t!("msg_0517")).size(11));
                if pass.report.ranges.is_empty() && !pass.report.quality.episodes.iter()
                    .any(|ep| ep.verdict == Verdict::Review) {
                    content = content.push(text(crate::t!("msg_0515")).size(11));
                }
                if !pass.report.ranges.is_empty() {
                    content = content.push(text(crate::t!("msg_0521")).size(12));
                    let mut items = column![].spacing(6);
                    for (index, range) in pass.report.ranges.iter().enumerate() {
                        let label = crate::tf!("msg_0448",
                            segment=range.segment + 1, start=range.start + 1,
                            end=range.end + 1, count=range.count());
                        let focused = pass.focused == Some((false, index));
                        items = items.push(container(column![
                            checkbox(pass.selected.contains(&index))
                                .label(label)
                                .on_toggle(move |checked| Message::InspectionToggleExtra(
                                    pass_index, index, false, checked))
                                .size(14),
                            text(original_for_numbers.and_then(|original| extra_number_map.as_ref()
                                .and_then(|map| original_number_label(map, original,
                                    range.segment, range.start, range.end)))
                                .unwrap_or_default()).size(11),
                            text(crate::tf!("msg_0449",
                                entry=format_jump_distance(range.entry_m),
                                exit=format_jump_distance(range.exit_m),
                                bridge=format_jump_distance(range.bridge_m))).size(11),
                            self.btn_secondary(crate::t!("msg_0450"))
                                .on_press(Message::InspectionFocusExtra(pass_index, index, false)),
                        ].spacing(4)).padding(6).width(Length::Fill)
                            .style(move |theme: &iced::Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(if focused { palette.primary.weak.color }
                                        else { palette.background.weak.color }.into()),
                                    ..Default::default()
                                }
                            }));
                    }
                    content = content.push(items);
                }
                if pass.report.quality.episodes.iter().any(|ep| ep.verdict == Verdict::Review) {
                    content = content.push(text(crate::t!("msg_0522")).size(12));
                    let mut items = column![].spacing(6);
                    for (index, ep) in pass.report.quality.episodes.iter().enumerate() {
                        if ep.verdict != Verdict::Review { continue; }
                        let label = crate::tf!("msg_0491",
                            segment=ep.segment + 1, start=ep.start + 1,
                            end=ep.end + 1, count=ep.count());
                        let focused = pass.focused == Some((true, index));
                        items = items.push(container(column![
                            checkbox(pass.selected_reviews.contains(&index))
                                .label(label)
                                .on_toggle(move |checked| Message::InspectionToggleExtra(
                                    pass_index, index, true, checked))
                                .size(14),
                            text(original_for_numbers.and_then(|original| extra_number_map.as_ref()
                                .and_then(|map| original_number_label(map, original,
                                    ep.segment, ep.start, ep.end)))
                                .unwrap_or_default()).size(11),
                            text(crate::tf!("msg_0494",
                                bad=ep.bad_fixes, jumps=ep.suspect_steps,
                                distance=format_jump_distance(ep.internal_m),
                                bridge=if ep.bridge_m.is_finite() {
                                    format_jump_distance(ep.bridge_m)
                                } else { "—".to_string() })).size(11),
                            self.btn_secondary(crate::t!("msg_0450"))
                                .on_press(Message::InspectionFocusExtra(pass_index, index, true)),
                        ].spacing(4)).padding(6).width(Length::Fill)
                            .style(move |theme: &iced::Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(if focused { palette.primary.weak.color }
                                        else { palette.background.weak.color }.into()),
                                    ..Default::default()
                                }
                            }));
                    }
                    content = content.push(items);
                }
            }
            let can_scan_more = inspect.extra_passes.len() < 3 &&
                if let Some(last) = inspect.extra_passes.last() {
                    !last.selected.is_empty() || !last.selected_reviews.is_empty()
                } else {
                    !inspect.second_selected.is_empty()
                        || !inspect.second_selected_reviews.is_empty()
                };
            if can_scan_more {
                content = content.push(
                    self.btn_secondary(crate::t!("msg_0518"))
                        .on_press(Message::InspectionScanAgain),
                );
                content = content.push(text(crate::t!("msg_0519")).size(11));
            }
        }

        if let Some(segments) = &inspect.preview_segments {
            let remaining: usize = segments.iter().map(|s|s.points.len()).sum();
            let distance: f64 = segments.iter().flat_map(|s|s.points.windows(2))
                .map(|p|crate::domain::haversine_m(p[0].lat,p[0].lon,p[1].lat,p[1].lon)).sum();
            content = content.push(text(crate::tf!("msg_0496", removed=report.points.saturating_sub(remaining),
                remaining=remaining, segments=segments.len(), distance=format!("{:.2}",distance/1000.0))).size(12));
            if let Some((suggested, review)) = inspect.preview_residual {
                content = content.push(text(crate::tf!("msg_0509",
                    suggested = suggested, review = review)).size(11));
            }
            if !report.can_reorder && remaining * 4 < report.points {
                content = content.push(text(crate::t!("msg_0497")).size(12));
            }
        }
        let can_preview = report.can_reorder || !inspect.selected.is_empty() || !inspect.selected_reviews.is_empty();
        // Require an explicit preview before applying either type of change.
        let can_apply = can_preview && inspect.preview_segments.as_ref()
            .is_some_and(|segments| segments.iter().any(|s| !s.points.is_empty()));
        let preview_label = if inspect.preview_segments.is_some() {
            crate::t!("msg_0454")
        } else { crate::t!("msg_0453") };
        let apply_label = if report.can_reorder {
            crate::t!("msg_0455")
        } else { crate::t!("msg_0456") };
        let buttons = column![
            self.btn_secondary(preview_label).width(Length::Fill).on_press_maybe(
                can_preview.then_some(Message::InspectionPreview)),
            self.btn(apply_label).width(Length::Fill).on_press_maybe(
                can_apply.then_some(Message::InspectionCreateCopy)),
            self.btn_secondary(crate::t!("msg_0457")).width(Length::Fill)
                .on_press(Message::InspectionClose),
        ].spacing(8);
        column![
            row![
                text(crate::t!("msg_0436")).size(16),
                Space::new().width(Length::Fill),
                self.icon_button(lucide_iced::bytes::X, Message::InspectionClose),
            ].align_y(Alignment::Center),
            scrollable(content.padding(4)).height(Length::Fill),
            buttons,
        ].spacing(9).padding(10).width(Length::Fill).height(Length::Fill).into()
    }
}
