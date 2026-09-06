//! Hand the run to torena-hub's Skill Planner, as a link.
//!
//! # Why a link and not a file
//!
//! The planner already accepts a session on its query string:
//! `/skill-planner?planner=<code>` decodes a v1 payload and hydrates runner,
//! obtained skills, budget and hint discounts in one step (`usePlannerImport`
//! in torena-hub). So there is nothing to write, nothing to import by hand,
//! and no second format to keep in sync — the plugin builds the same code the
//! web app builds, and opens the page with it.
//!
//! # The wire format is theirs
//!
//! The bit layout is specified in torena-hub's
//! `src/modules/skill-planner/share/ENCODING.md` (version 1) and implemented in
//! `share/encoding.ts` over `runners/share/bit-vector.ts`. This is a writer for
//! that format and nothing more: no reader, because nothing here decodes, and
//! no fields of our own, because a field the planner does not read is a field
//! that will drift. [`tests::matches_the_typescript_encoder`] pins the two
//! implementations together with codes the TypeScript one produced.
//!
//! # Where the work happens
//!
//! A click lands on the render thread, the reads have to happen on the Unity
//! main thread, and opening a browser should happen on neither. [`request`]
//! makes that split, the same way [`crate::veterans_export`] does.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::compat::Sdk;
use crate::memory_reader::{AcquiredSkillInfo, CareerSnapshot, SkillTip};

/// The wire format version this writes. Bump only alongside torena-hub.
const VERSION: u8 = 1;

/// "Studious" — the 10% skill-cost discount, which the planner calls Fast
/// Learner. Confirmed against the game's own master data (`skill_data` row
/// 201432, `text_data` category 47).
const FAST_LEARNER_SKILL_ID: i32 = 201432;

/// Encoding caps. Anything past them is dropped rather than wrapped, because
/// the count fields cannot describe more.
const MAX_OBTAINED: usize = 63;
const MAX_CANDIDATES: usize = 127;

/// Set while an export is in flight, so a double-click is one export.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// A skill offered with a hint discount, as the planner wants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub skill_id: i32,
    /// `0`–`5`; higher is a bigger discount.
    pub hint_level: u8,
}

/// One planner session, in the encoding's own field names.
///
/// Deliberately a plain data struct with no reader: it is built from a career,
/// encoded, and thrown away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlannerExport {
    pub card_id: i32,
    pub speed: i32,
    pub stamina: i32,
    pub power: i32,
    pub guts: i32,
    pub wiz: i32,
    /// The ten aptitude grades, in wire order: distance short/mile/middle/long,
    /// ground turf/dirt, style nige/senko/sashi/oikomi.
    pub aptitudes: [i32; 10],
    /// `1`–`5`, Front Runner through Runaway.
    pub strategy: i32,
    /// `-2`–`+2`, Awful through Great. Encoded with the `+2` offset.
    pub mood: i32,
    /// Skill points available to spend.
    pub budget: i32,
    pub fast_learner: bool,
    pub obtained_skills: Vec<i32>,
    pub candidate_skills: Vec<Candidate>,
}

impl PlannerExport {
    /// Encode to the URL-safe Base64 the planner's `?planner=` param takes.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut bits = BitWriter::default();

        bits.write(u32::from(VERSION), 8);
        bits.write(clamp(self.card_id, 0, 0x000F_FFFF), 20);

        for stat in [self.speed, self.stamina, self.power, self.guts, self.wiz] {
            bits.write(clamp(stat, 0, 2047), 11);
        }
        for apt in self.aptitudes {
            bits.write(clamp(apt, 0, 9), 4);
        }

        // Out-of-range values take the decoder's own defaults rather than a
        // clamp to the nearest end: an unknown strategy is Front Runner and an
        // unknown mood is Great, both sides.
        bits.write(
            if (1..=5).contains(&self.strategy) {
                self.strategy as u32
            } else {
                1
            },
            3,
        );
        let mood = self.mood + 2;
        bits.write(if (0..=4).contains(&mood) { mood as u32 } else { 4 }, 3);

        bits.write(clamp(self.budget, 0, 65535), 16);
        bits.write(u32::from(self.fast_learner), 1);

        let obtained = &self.obtained_skills[..self.obtained_skills.len().min(MAX_OBTAINED)];
        bits.write(obtained.len() as u32, 6);
        for &skill_id in obtained {
            bits.write(clamp(skill_id, 0, 0x000F_FFFF), 20);
        }

        let candidates = &self.candidate_skills[..self.candidate_skills.len().min(MAX_CANDIDATES)];
        bits.write(candidates.len() as u32, 7);
        for candidate in candidates {
            bits.write(clamp(candidate.skill_id, 0, 0x000F_FFFF), 20);
            bits.write(u32::from(candidate.hint_level.min(5)), 3);
        }

        bits.finish()
    }

    /// The full planner URL for this session.
    #[must_use]
    pub fn url(&self, base: &str) -> String {
        format!("{}?planner={}", base.trim_end_matches('?'), self.encode())
    }
}

/// Assemble a session from a settled capture plus the three things that move
/// while you shop: the balance, what you have learned, and your hints.
///
/// Aptitudes are sent per-field even though the planner collapses them to one
/// value per group on import — the encoding has the room, and sending the real
/// grades costs nothing.
#[must_use]
pub fn from_career(
    snapshot: &CareerSnapshot,
    budget: i32,
    skills: &[AcquiredSkillInfo],
    tips: &[SkillTip],
) -> PlannerExport {
    let apt = &snapshot.aptitudes;
    let aptitudes = [
        apt.dist_short,
        apt.dist_mile,
        apt.dist_middle,
        apt.dist_long,
        apt.ground_turf,
        apt.ground_dirt,
        apt.style_nige,
        apt.style_senko,
        apt.style_sashi,
        apt.style_oikomi,
    ];
    let obtained_skills: Vec<i32> = skills.iter().map(|s| s.master_id).collect();

    PlannerExport {
        card_id: snapshot.card_id,
        speed: snapshot.speed,
        stamina: snapshot.stamina,
        power: snapshot.power,
        guts: snapshot.guts,
        wiz: snapshot.wiz,
        aptitudes,
        strategy: strategy_from_aptitudes(apt),
        mood: mood_from_motivation(snapshot.motivation),
        budget,
        fast_learner: obtained_skills.contains(&FAST_LEARNER_SKILL_ID),
        candidate_skills: crate::gametora_data::hinted_skills(tips)
            .into_iter()
            .map(|(skill_id, hint_level)| Candidate { skill_id, hint_level })
            .collect(),
        obtained_skills,
    }
}

/// `RaceDefine.Motivation` is `1`–`5` (Awful through Great); the planner's mood
/// is `-2`–`+2`. Anything outside the enum reads as Normal rather than as an
/// invented extreme.
fn mood_from_motivation(motivation: i32) -> i32 {
    if (1..=5).contains(&motivation) {
        motivation - 3
    } else {
        0
    }
}

/// The style the runner is best at, as the planner's strategy id.
///
/// A career has no settled running style — it is chosen per race — so this is
/// the honest guess rather than a reading. It only affects which skills the
/// planner highlights, and the aptitudes it is derived from are sent alongside
/// it, so a user who disagrees can change it in one click.
fn strategy_from_aptitudes(apt: &crate::evaluation::Aptitudes) -> i32 {
    // Front Runner, Pace Chaser, Late Surger, End Closer — the planner's own
    // order, which is also the game's.
    let styles = [
        (1, apt.style_nige),
        (2, apt.style_senko),
        (3, apt.style_sashi),
        (4, apt.style_oikomi),
    ];
    styles
        .iter()
        .max_by_key(|(id, grade)| (*grade, -id))
        .map_or(1, |(id, _)| *id)
}

/// Kick off an export. Safe to call from the render thread (which is where a
/// panel click lands): the reads are scheduled onto the Unity main thread, and
/// the browser is opened from neither.
pub fn request() {
    if RUNNING.swap(true, Ordering::AcqRel) {
        hlog_info!(target: "training-tracker", "Skill planner export already running");
        return;
    }
    Sdk::get().schedule_on_main_thread(export_cb);
}

/// Unity main thread: read the three live things, then hand off.
///
/// Only `WorkSingleModeCharaData` is touched — the same career-lifetime work
/// data the shop-screen light refresh reads, not per-screen UI objects.
extern "C" fn export_cb() {
    let snapshot = crate::ui::latest_snapshot();
    let budget = crate::memory_reader::read_skill_points().unwrap_or(0);
    let skills = crate::memory_reader::read_acquired_skills();
    let tips = crate::memory_reader::read_skill_tips();

    std::thread::spawn(move || {
        finish(snapshot.as_ref(), budget, &skills, &tips);
        RUNNING.store(false, Ordering::Release);
    });
}

/// Encode, copy, open, and say what happened. Off the game's threads.
fn finish(snapshot: Option<&CareerSnapshot>, budget: i32, skills: &[AcquiredSkillInfo], tips: &[SkillTip]) {
    let sdk = Sdk::get();
    let Some(snapshot) = snapshot else {
        hlog_warn!(target: "training-tracker", "Skill planner: no career capture to export");
        sdk.show_notification("Skill planner: no career loaded");
        return;
    };

    let export = from_career(snapshot, budget, skills, tips);
    // Hints the catalogue could not place are the one silent failure worth
    // naming: the planner would price every skill at full cost and look right
    // doing it.
    let unmapped_hints = !tips.is_empty() && export.candidate_skills.is_empty();
    if unmapped_hints {
        hlog_warn!(
            target: "training-tracker",
            "Skill planner: {} hint(s) read but none matched the skill catalogue (gametora data available: {})",
            tips.len(),
            crate::gametora_data::is_available()
        );
    }
    let url = export.url(&crate::config::read(|file| file.settings.skill_planner_url.clone()));

    let copied = honse_services::shell::copy_text(&url);
    let opened = honse_services::shell::open_url(&url);
    hlog_info!(
        target: "training-tracker",
        "Skill planner: {} obtained, {} hinted, {} SP (copied={copied}, opened={opened})",
        export.obtained_skills.len(),
        export.candidate_skills.len(),
        export.budget
    );

    sdk.show_notification(match (opened, copied, unmapped_hints) {
        (true, _, false) => "Skill planner opened in your browser",
        (true, _, true) => "Skill planner opened — hint discounts are missing",
        (false, true, _) => "Skill planner link copied — paste it in your browser",
        (false, false, _) => "Skill planner: could not open or copy the link",
    });
}

/// Clamp into the range a field of the encoding can hold.
fn clamp(value: i32, low: i32, high: i32) -> u32 {
    value.clamp(low, high) as u32
}

/// Big-endian bit packer with the encoding's URL-safe Base64 tail.
///
/// A port of torena-hub's `BitVector`, writer half only. Bits go in
/// most-significant first, and the tail is zero-padded to a multiple of six —
/// which is why a decoder has to guard on bits remaining rather than trust the
/// length.
#[derive(Default)]
struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    fn write(&mut self, value: u32, bit_length: u32) {
        for i in (0..bit_length).rev() {
            self.bits.push(((value >> i) & 1) as u8);
        }
    }

    fn finish(mut self) -> String {
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        while !self.bits.len().is_multiple_of(6) {
            self.bits.push(0);
        }
        self.bits
            .chunks(6)
            .map(|chunk| {
                let index = chunk.iter().fold(0usize, |acc, &bit| (acc << 1) | bit as usize);
                ALPHABET[index] as char
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::Aptitudes;

    fn session() -> PlannerExport {
        PlannerExport {
            card_id: 100_601,
            speed: 1200,
            stamina: 800,
            power: 600,
            guts: 400,
            wiz: 900,
            aptitudes: [3, 7, 8, 5, 8, 4, 7, 6, 5, 3],
            strategy: 3,
            mood: 2,
            budget: 1500,
            fast_learner: true,
            obtained_skills: vec![201_432, 200_061],
            candidate_skills: vec![
                Candidate {
                    skill_id: 200_062,
                    hint_level: 3,
                },
                Candidate {
                    skill_id: 200_064,
                    hint_level: 5,
                },
                Candidate {
                    skill_id: 201_431,
                    hint_level: 0,
                },
            ],
        }
    }

    /// The contract with the other side of the link.
    ///
    /// Both codes were produced by torena-hub's own `encodeSkillPlanner`
    /// (`src/modules/skill-planner/share/encoding.ts`) from these exact
    /// sessions. If this fails, the two implementations have drifted and the
    /// planner will read something other than what the game showed.
    #[test]
    fn matches_the_typescript_encoder() {
        let minimal = PlannerExport {
            card_id: 1,
            strategy: 1,
            ..PlannerExport::default()
        };
        assert_eq!(minimal.encode(), "AQAAEAAAAAAAAAAAAAAABQAAAAA");
        assert_eq!(session().encode(), "ARiPmWDIEsGQcIbwsI7KbgLuQjEtgw19BmGvzMNgKYlrg");
    }

    #[test]
    fn the_url_carries_the_code_on_the_param_the_planner_reads() {
        let url = session().url("https://torena-sim.pages.dev/skill-planner");
        assert_eq!(
            url,
            format!(
                "https://torena-sim.pages.dev/skill-planner?planner={}",
                session().encode()
            )
        );
    }

    /// Out-of-range input must not shift every field after it.
    #[test]
    fn oversized_values_are_clamped_not_wrapped() {
        let wild = PlannerExport {
            card_id: 1,
            speed: 9999,
            aptitudes: [99; 10],
            budget: 999_999,
            strategy: 42,
            mood: 17,
            ..PlannerExport::default()
        };
        // Same length as the minimal payload: 159 bits either way.
        assert_eq!(wild.encode().len(), 27);
    }

    #[test]
    fn only_the_first_63_obtained_and_127_candidates_are_encoded() {
        let mut export = PlannerExport {
            card_id: 1,
            strategy: 1,
            obtained_skills: vec![200_011; 70],
            ..PlannerExport::default()
        };
        export.candidate_skills = vec![
            Candidate {
                skill_id: 200_011,
                hint_level: 1,
            };
            200
        ];
        // 159 + 63*20 + 127*23 = 4340 bits → 724 base64 characters.
        assert_eq!(export.encode().len(), 724);
    }

    #[test]
    fn motivation_maps_onto_the_planner_mood_range() {
        assert_eq!(mood_from_motivation(1), -2); // Awful
        assert_eq!(mood_from_motivation(3), 0); // Normal
        assert_eq!(mood_from_motivation(5), 2); // Great
        assert_eq!(mood_from_motivation(0), 0); // unread → Normal, not Awful
        assert_eq!(mood_from_motivation(9), 0);
    }

    #[test]
    fn strategy_follows_the_best_style_aptitude() {
        let apt = Aptitudes {
            style_sashi: 7,
            style_nige: 5,
            ..Aptitudes::default()
        };
        assert_eq!(strategy_from_aptitudes(&apt), 3); // Late Surger

        // A tie goes to the earlier style, so an unread career reads as Front
        // Runner rather than as End Closer.
        let flat = Aptitudes::default();
        assert_eq!(strategy_from_aptitudes(&flat), 1);
    }
}
