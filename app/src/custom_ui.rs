//! Training page: custom schedules (import shared `.fmf` files, edit, assign to coming weeks).
//! The data lives in `PlannerSettings.custom`; the live writer applies it.

use chrono::NaiveDate;
use eframe::egui::{self, RichText, Ui};
use std::path::PathBuf;

use crate::custom::{CustomSchedule, CustomTraining, Source};
use crate::model::{self, Category};
use crate::t;
use crate::theme::{self, icon};

#[derive(Default)]
pub struct UiState {
    /// Schedule whose grid is open for editing.
    editing: Option<String>,
    /// Text in the rename field while editing.
    rename: String,
    /// How many weeks "Use for the next weeks" covers.
    weeks: u32,
    pub message: String,
    pub error: bool,
}

/// What the page has to do after the card was drawn.
#[derive(Default)]
pub struct Outcome {
    /// Settings changed: save them and preview again.
    pub changed: bool,
}

/// Import `.fmf` files (from the picker or dropped on the window). Returns a message for the user.
pub fn import(c: &mut CustomTraining, paths: &[PathBuf]) -> (String, bool) {
    let (mut ok, mut errors) = (Vec::new(), Vec::new());
    for p in paths {
        match crate::fmf::import_file(p) {
            Ok(r) => {
                let mut s = r.schedule;
                if c.schedules.iter().any(|x| x.days == s.days) {
                    ok.push(t!(format!("{} (stond er al)", s.name), format!("{} (already there)", s.name)));
                    continue;
                }
                if c.get(&s.name).is_some() {
                    s.name = (2..).map(|i| format!("{} ({i})", s.name)).find(|n| c.get(n).is_none()).unwrap_or_default();
                }
                let mut line = s.name.clone();
                if !r.unknown.is_empty() {
                    line += &t!(format!(" — onbekende sessies {:?} blijven van FM", r.unknown), format!(" — unknown sessions {:?} are left to FM", r.unknown));
                }
                ok.push(line);
                c.schedules.push(s);
            }
            Err(e) => errors.push(format!("{}: {e:#}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
        }
    }
    let mut msg = String::new();
    if !ok.is_empty() {
        msg = t!(format!("Geïmporteerd: {}.", ok.join(", ")), format!("Imported: {}.", ok.join(", ")));
    }
    if !errors.is_empty() {
        if !msg.is_empty() {
            msg.push(' ');
        }
        msg += &t!(format!("Niet gelukt: {}", errors.join("; ")), format!("Failed: {}", errors.join("; ")));
    }
    (msg, !errors.is_empty())
}

fn pick_files() -> Vec<PathBuf> {
    let mut d = rfd::FileDialog::new().add_filter("FM training schedule", &["fmf"]).set_title("FM27 Trainer");
    let schedules = crate::fm_user_dir().join("schedules");
    if schedules.is_dir() {
        d = d.set_directory(schedules);
    }
    d.pick_files().unwrap_or_default()
}

/// The card with all custom schedules. `today` is FM's date (None without a connection).
pub fn card(ui: &mut Ui, c: &mut CustomTraining, st: &mut UiState, today: Option<NaiveDate>) -> Outcome {
    let mut out = Outcome::default();
    if st.weeks == 0 {
        st.weeks = 4;
    }
    theme::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::ic(icon::LIST_CHECKS).color(theme::ACCENT).size(16.0));
            ui.label(theme::h2(t!("Eigen schema's", "Custom schedules")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::button(ui, icon::PLUS, t!("Nieuw", "New"), true).clicked() {
                    let name = c.free_name(t!("Schema", "Schedule"));
                    let mut s = CustomSchedule::new(&name);
                    s.days = [[Some(300); 3]; 7];
                    c.schedules.push(s);
                    st.editing = Some(name.clone());
                    st.rename = name;
                    out.changed = true;
                }
                if theme::primary_button(ui, icon::FILE_ARROW_DOWN, t!("Importeer .fmf…", "Import .fmf…"), true).clicked() {
                    let files = pick_files();
                    if !files.is_empty() {
                        (st.message, st.error) = import(c, &files);
                        out.changed = true;
                    }
                }
            });
        });
        ui.add(
            egui::Label::new(theme::dim(t!(
                "Gebruik schema's die mensen online delen (.fmf uit FM's trainingsscherm): importeer ze, of sleep ze op dit venster. Geef ze daarna aan de komende weken; de app zet ze in FM's kalender en houdt ze daar vast. Wedstrijd- en reisdagen blijven van FM.",
                "Use schedules people share online (.fmf exported from FM's training screen): import them, or drop them on this window. Then give them to the coming weeks; the app writes them into FM's calendar and keeps them there. Match and travel days stay with FM."
            )))
            .wrap(),
        );
        if !st.message.is_empty() {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(&st.message).color(if st.error { theme::DANGER } else { theme::SUCCESS })).wrap());
        }
        if c.schedules.is_empty() {
            return;
        }
        ui.add_space(8.0);

        // standaard voor alle weken zonder eigen keuze
        ui.horizontal(|ui| {
            ui.label(t!("Elke week zonder eigen keuze:", "Every week without its own choice:"));
            let auto = t!("Automatische planner", "Automatic planner").to_string();
            let cur = c.default.clone().unwrap_or_else(|| auto.clone());
            egui::ComboBox::from_id_salt("custom_default").selected_text(cur).width(220.0).show_ui(ui, |ui| {
                if ui.selectable_label(c.default.is_none(), &auto).clicked() {
                    c.default = None;
                    out.changed = true;
                }
                for n in c.schedules.iter().map(|s| s.name.clone()).collect::<Vec<_>>() {
                    if ui.selectable_label(c.default.as_deref() == Some(n.as_str()), &n).clicked() {
                        c.default = Some(n);
                        out.changed = true;
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(t!("Gebruik voor", "Use for"));
            ui.add(egui::DragValue::new(&mut st.weeks).range(1..=52));
            ui.label(t!("weken vanaf deze week", "weeks from this week on"));
            if today.is_none() {
                ui.label(theme::faint(t!("(eerst met FM verbinden)", "(connect to FM first)")));
            }
        });
        ui.add_space(6.0);
        out.changed |= pattern(ui, c, today);
        ui.add_space(6.0);

        let mut remove = None;
        let names: Vec<String> = c.schedules.iter().map(|s| s.name.clone()).collect();
        for name in names {
            ui.separator();
            let editing = st.editing.as_deref() == Some(name.as_str());
            let weeks_using = c.assigned.values().filter(|v| **v == name).count();
            ui.horizontal(|ui| {
                let desc = c.get(&name).map(|s| s.description.clone()).unwrap_or_default();
                let l = ui.label(theme::label_strong(&name));
                if !desc.is_empty() {
                    l.on_hover_text(desc);
                }
                if c.default.as_deref() == Some(name.as_str()) {
                    theme::pill(ui, t!("standaard", "default"), theme::ACCENT);
                }
                if weeks_using > 0 {
                    theme::pill(ui, &if weeks_using == 1 { t!("1 week", "1 week").to_string() } else { t!(format!("{weeks_using} weken"), format!("{weeks_using} weeks")) }, theme::SUCCESS);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::small_button(ui, &format!("{} {}", icon::TRASH, t!("Verwijder", "Delete")), true).clicked() {
                        remove = Some(name.clone());
                    }
                    let edit_label = if editing { format!("{} {}", icon::CHECK, t!("Klaar", "Done")) } else { format!("{} {}", icon::PENCIL_SIMPLE, t!("Bewerk", "Edit")) };
                    if theme::small_button(ui, &edit_label, true).clicked() {
                        if editing {
                            st.editing = None;
                        } else {
                            st.editing = Some(name.clone());
                            st.rename = name.clone();
                        }
                    }
                    if let Some(t0) = today {
                        let label = format!("{} {}", icon::CALENDAR_PLUS, t!(format!("Gebruik {} weken", st.weeks), format!("Use for {} weeks", st.weeks)));
                        if theme::small_button(ui, &label, true).clicked() {
                            c.assign(crate::planner::monday_of(t0), st.weeks, Some(&name));
                            st.message = t!(
                                format!("{name} staat nu op {} weken vanaf {}.", st.weeks, crate::i18n::date_short(crate::planner::monday_of(t0))),
                                format!("{name} is now set for {} weeks from {}.", st.weeks, crate::i18n::date_short(crate::planner::monday_of(t0)))
                            );
                            st.error = false;
                            out.changed = true;
                        }
                    }
                });
            });
            let Some(s) = c.schedules.iter_mut().find(|s| s.name == name) else { continue };
            if editing {
                out.changed |= editor(ui, s);
                // hernoemen: ook de toewijzingen gaan mee
                ui.horizontal(|ui| {
                    ui.label(t!("Naam", "Name"));
                    ui.text_edit_singleline(&mut st.rename);
                    let new = st.rename.trim().to_string();
                    let free = !new.is_empty() && new != name && !c.schedules.iter().any(|x| x.name == new);
                    if theme::small_button(ui, t!("Hernoem", "Rename"), free).clicked() {
                        c.rename(&name, &new);
                        st.editing = Some(new);
                        out.changed = true;
                    }
                });
            } else {
                preview(ui, s);
            }
        }
        if let Some(n) = remove {
            c.remove(&n);
            out.changed = true;
        }
    });
    out
}

/// Seven columns with the three sessions per day.
fn preview(ui: &mut Ui, s: &CustomSchedule) {
    ui.columns(7, |cols| {
        for (d, ui) in cols.iter_mut().enumerate() {
            ui.label(theme::caps(crate::i18n::weekday_short(d)));
            for slot in s.days[d] {
                match slot.and_then(model::session_by_code) {
                    Some(n) => {
                        theme::session_chip(ui, &short(n), theme::rgb(model::category_of(n).color()), false).on_hover_text(n);
                    }
                    None => {
                        ui.label(theme::faint(t!("FM's keuze", "FM's choice")));
                    }
                }
            }
        }
    });
}

fn editor(ui: &mut Ui, s: &mut CustomSchedule) -> bool {
    let mut changed = false;
    ui.columns(7, |cols| {
        for (d, ui) in cols.iter_mut().enumerate() {
            ui.label(theme::caps(crate::i18n::weekday_short(d)));
            for i in 0..3 {
                changed |= session_combo(ui, &format!("cs_{}_{d}_{i}", s.name), &mut s.days[d][i]);
            }
        }
    });
    ui.horizontal(|ui| {
        changed |= ui
            .checkbox(&mut s.follow_match_rhythm, t!("Dagen rond wedstrijden van de automatische planner", "Days around matches from the automatic planner"))
            .changed();
    });
    ui.add(
        egui::Label::new(theme::faint(t!(
            "Aan: de dag voor en na een wedstrijd krijgen het ritme van de planner (Recovery, Match Preparation), zodat het schema ook werkt als de wedstrijd op een andere dag valt. Uit: het schema precies zoals het is.",
            "On: the day before and after a match get the planner's rhythm (Recovery, Match Preparation), so the schedule also works when the match is on another day. Off: the schedule exactly as it is."
        )))
        .wrap(),
    );
    changed
}

fn session_combo(ui: &mut Ui, id: &str, slot: &mut Option<u16>) -> bool {
    let mut changed = false;
    let text = slot.and_then(model::session_by_code).map(short).unwrap_or_else(|| t!("FM's keuze", "FM's choice").to_string());
    egui::ComboBox::from_id_salt(id).selected_text(text).width(ui.available_width()).show_ui(ui, |ui| {
        if ui.selectable_label(slot.is_none(), t!("FM's keuze (niet aanpassen)", "FM's choice (leave as is)")).clicked() {
            *slot = None;
            changed = true;
        }
        for cat in [Category::TacticalUnderstanding, Category::MatchPreparation, Category::FootballingDevelopment, Category::PhysicalRecovery, Category::ExtraCurricular] {
            ui.separator();
            ui.label(RichText::new(cat.label()).small().color(theme::rgb(cat.color())));
            for sess in model::SESSIONS.iter().filter(|x| x.category == cat) {
                let Some(code) = model::session_code(sess.name).filter(|c| crate::custom::selectable(*c)) else { continue };
                if ui.selectable_label(*slot == Some(code), sess.name).clicked() {
                    *slot = Some(code);
                    changed = true;
                }
            }
        }
    });
    changed
}

fn short(n: &str) -> String {
    n.replace("Out Of Possession", "OOP")
        .replace("In Possession", "IP")
        .replace("Defensive", "Def.")
        .replace("Attacking", "Att.")
        .replace("Conditioning", "Cond.")
        .replace("Individual Development", "Individual")
}

/// Choice of schedule on a week card. Returns true when it changed.
pub fn week_choice(ui: &mut Ui, c: &mut CustomTraining, monday: NaiveDate) -> bool {
    if c.schedules.is_empty() {
        return false;
    }
    let mut changed = false;
    let own = c.assigned.get(&monday).cloned();
    // wat de week krijgt zonder eigen keuze: patroon, standaard of de automatische planner
    let mut without = c.clone();
    without.assigned.remove(&monday);
    let default = match without.source(monday) {
        Some((s, Source::Pattern(i))) => t!(format!("Patroon {}/{}: {}", i + 1, c.pattern.len(), s.name), format!("Pattern {}/{}: {}", i + 1, c.pattern.len(), s.name)),
        Some((s, _)) => t!(format!("Standaard ({})", s.name), format!("Default ({})", s.name)),
        None => t!("Automatisch", "Automatic").to_string(),
    };
    let follows_something = without.source(monday).is_some();
    let only_auto = t!("Automatische planner (alleen deze week)", "Automatic planner (this week only)");
    let text = match own.as_deref() {
        Some("") => only_auto.to_string(),
        Some(n) => n.to_string(),
        None => default.clone(),
    };
    egui::ComboBox::from_id_salt(("week_schedule", monday)).selected_text(text).width(190.0).show_ui(ui, |ui| {
        if ui.selectable_label(own.is_none(), &default).clicked() {
            c.assign(monday, 1, None);
            changed = true;
        }
        if follows_something && ui.selectable_label(own.as_deref() == Some(""), only_auto).clicked() {
            // een eigen toewijzing die naar geen schema wijst = automatisch
            c.assigned.insert(monday, String::new());
            changed = true;
        }
        for n in c.schedules.iter().map(|s| s.name.clone()).collect::<Vec<_>>() {
            if ui.selectable_label(own.as_deref() == Some(n.as_str()), &n).clicked() {
                c.assign(monday, 1, Some(&n));
                changed = true;
            }
        }
    });
    changed
}

/// Save what FM has in a week as a new schedule.
pub fn save_week(c: &mut CustomTraining, start: NaiveDate, codes: &[[u16; 3]]) -> String {
    let name = c.free_name(&t!(format!("Week {}", crate::i18n::date_short(start)), format!("Week {}", crate::i18n::date_short(start))));
    c.schedules.push(CustomSchedule::from_codes(&name, codes));
    name
}

/// Repeating pattern: schedules week after week in a fixed order (C1, C2, C3, C4, C1, …).
fn pattern(ui: &mut Ui, c: &mut CustomTraining, today: Option<NaiveDate>) -> bool {
    let mut changed = false;
    let this_week = today.map(crate::planner::monday_of);
    ui.horizontal(|ui| {
        if theme::toggle(ui, &mut c.pattern_on).changed() {
            changed = true;
        }
        ui.label(theme::label_strong(t!("Herhalend patroon", "Repeating pattern")));
        ui.label(theme::faint(t!("schema's week na week in deze volgorde, daarna weer vooraan", "schedules week after week in this order, then from the start again")));
    });
    // het patroon begint deze week als er nog geen begin is
    if c.pattern_on && c.pattern_start.is_none() {
        if let Some(w) = this_week {
            c.pattern_start = Some(w);
            changed = true;
        }
    }
    if !c.pattern_on {
        return changed;
    }
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        let mut swap = None;
        for (i, n) in c.pattern.iter().enumerate() {
            theme::pill(ui, &format!("{}. {n}", i + 1), theme::ACCENT);
            if i > 0 && theme::small_button(ui, "←", true).on_hover_text(t!("Eerder", "Earlier")).clicked() {
                swap = Some(i);
            }
            if theme::small_button(ui, &icon::X.to_string(), true).on_hover_text(t!("Uit het patroon", "Remove from the pattern")).clicked() {
                remove = Some(i);
            }
            ui.add_space(6.0);
        }
        if let Some(i) = swap {
            c.pattern.swap(i - 1, i);
            changed = true;
        }
        if let Some(i) = remove {
            c.pattern.remove(i);
            changed = true;
        }
        egui::ComboBox::from_id_salt("pattern_add").selected_text(format!("{} {}", icon::PLUS, t!("Voeg toe", "Add"))).show_ui(ui, |ui| {
            for n in c.schedules.iter().map(|s| s.name.clone()).collect::<Vec<_>>() {
                if ui.selectable_label(false, &n).clicked() {
                    c.pattern.push(n);
                    changed = true;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        match c.pattern_start {
            Some(st) => {
                ui.label(theme::dim(t!(format!("Begint in de week van {}", crate::i18n::date_short(st)), format!("Starts in the week of {}", crate::i18n::date_short(st)))));
            }
            None => {
                ui.label(theme::faint(t!("Begint deze week zodra FM verbonden is.", "Starts this week once FM is connected.")));
            }
        }
        if let Some(w) = this_week {
            if c.pattern_start != Some(w) && theme::small_button(ui, t!("Begin deze week opnieuw", "Restart this week"), true).clicked() {
                c.pattern_start = Some(w);
                changed = true;
            }
        }
    });
    // de komende weken volgens het patroon
    if let (Some(w), false) = (this_week, c.pattern.is_empty()) {
        let next: Vec<String> = (0..c.pattern.len().clamp(4, 8))
            .map(|k| w + chrono::Duration::weeks(k as i64))
            .map(|m| {
                let name = c.source(m).map(|(s, _)| s.name.clone()).unwrap_or_else(|| t!("automatisch", "automatic").to_string());
                format!("{} {name}", crate::i18n::date_short(m))
            })
            .collect();
        ui.add(egui::Label::new(theme::faint(t!(format!("Komende weken: {}", next.join(" · ")), format!("Coming weeks: {}", next.join(" · "))))).wrap());
    }
    if !c.assigned.is_empty() {
        ui.add(
            egui::Label::new(theme::faint(t!(
                "Een week met een eigen keuze (op de weekkaart of via Gebruik N weken) gaat voor het patroon.",
                "A week with its own choice (on the week card or via Use for N weeks) goes before the pattern."
            )))
            .wrap(),
        );
    }
    changed
}
