//! The simulator's CW timing: when each character of a CW macro is on the air (spec §3.2).
//!
//! The simulator's own model, independent of Nexus's keyer timing (`tempo_core::cw`), so a test
//! can compare the two. PARIS timing: a dit is 1200/wpm ms, a dah three dits; one dit of silence
//! between the elements of a character, three between characters, seven between words. Inside a
//! macro `>` raises the speed by 5 wpm and `<` lowers it by 5, for the rest of the macro, and the
//! letters between two `|` are one character, a prosign, sent with no gap between them (spec
//! §3.2.1). A character with no Morse code takes no time: what a server does with one is not
//! documented.

/// One sent character (or prosign) and when it is on the air, in ms from the macro's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub text: String,
    pub start_ms: f64,
    pub end_ms: f64,
}

/// A macro's keying schedule.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schedule {
    pub marks: Vec<Mark>,
    /// From the first element's start to the last element's end.
    pub total_ms: f64,
}

/// The slowest speed `<` can reach.
const MIN_WPM: u32 = 5;

/// The Morse code of `c` (ITU, letters in any case), dots and dashes.
pub fn morse(c: char) -> Option<&'static str> {
    Some(match c.to_ascii_uppercase() {
        'A' => ".-",
        'B' => "-...",
        'C' => "-.-.",
        'D' => "-..",
        'E' => ".",
        'F' => "..-.",
        'G' => "--.",
        'H' => "....",
        'I' => "..",
        'J' => ".---",
        'K' => "-.-",
        'L' => ".-..",
        'M' => "--",
        'N' => "-.",
        'O' => "---",
        'P' => ".--.",
        'Q' => "--.-",
        'R' => ".-.",
        'S' => "...",
        'T' => "-",
        'U' => "..-",
        'V' => "...-",
        'W' => ".--",
        'X' => "-..-",
        'Y' => "-.--",
        'Z' => "--..",
        '0' => "-----",
        '1' => ".----",
        '2' => "..---",
        '3' => "...--",
        '4' => "....-",
        '5' => ".....",
        '6' => "-....",
        '7' => "--...",
        '8' => "---..",
        '9' => "----.",
        '.' => ".-.-.-",
        ',' => "--..--",
        '?' => "..--..",
        '\'' => ".----.",
        '!' => "-.-.--",
        '/' => "-..-.",
        '(' => "-.--.",
        ')' => "-.--.-",
        '&' => ".-...",
        ':' => "---...",
        ';' => "-.-.-.",
        '=' => "-...-",
        '+' => ".-.-.",
        '-' => "-....-",
        '_' => "..--.-",
        '"' => ".-..-.",
        '@' => ".--.-.",
        _ => return None,
    })
}

/// Dit units a code keys for, its inner gaps included.
fn units(code: &str) -> u32 {
    let on: u32 = code.chars().map(|e| if e == '-' { 3 } else { 1 }).sum();
    on + code.len().saturating_sub(1) as u32
}

/// The keying schedule of macro text (already unescaped) starting at `wpm`.
pub fn schedule(text: &str, wpm: u32) -> Schedule {
    let mut wpm = wpm.max(MIN_WPM);
    let mut out = Schedule::default();
    let mut t = 0.0f64;
    // Silence owed before the next character, in dits at the speed it is sent: 0 at the start,
    // 3 after a character, 7 after a word.
    let mut gap = 0u32;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let (label, code) = match c {
            '>' => {
                wpm += 5;
                continue;
            }
            '<' => {
                wpm = wpm.saturating_sub(5).max(MIN_WPM);
                continue;
            }
            ' ' => {
                if !out.marks.is_empty() {
                    gap = 7;
                }
                continue;
            }
            '|' => {
                // A prosign: everything to the next bar is one character.
                let mut label = String::new();
                let mut code = String::new();
                for p in chars.by_ref() {
                    if p == '|' {
                        break;
                    }
                    if let Some(m) = morse(p) {
                        label.push(p);
                        code.push_str(m);
                    }
                }
                if code.is_empty() {
                    continue;
                }
                (label, code)
            }
            other => match morse(other) {
                Some(m) => (other.to_string(), m.to_string()),
                None => continue,
            },
        };
        let dit = 1200.0 / f64::from(wpm);
        t += f64::from(gap) * dit;
        let start = t;
        t += f64::from(units(&code)) * dit;
        out.marks.push(Mark {
            text: label,
            start_ms: start,
            end_ms: t,
        });
        gap = 3;
    }
    out.total_ms = t;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paris_is_fifty_units_less_its_trailing_word_gap() {
        // PARIS plus its word space is 50 dits; with no trailing space it keys for 43.
        let s = schedule("PARIS", 20);
        assert!((s.total_ms - 43.0 * 60.0).abs() < 1e-9, "{}", s.total_ms);
        assert_eq!(s.marks.len(), 5);
        assert_eq!(s.marks[0].start_ms, 0.0);
        // P is .--. = 1+1+3+1+3+1+1 = 11 dits, then 3 of gap before A.
        assert!((s.marks[1].start_ms - 14.0 * 60.0).abs() < 1e-9);
    }

    #[test]
    fn a_word_gap_is_seven_dits_and_leading_spaces_cost_nothing() {
        let one = schedule("E E", 20).marks;
        assert!((one[1].start_ms - (1.0 + 7.0) * 60.0).abs() < 1e-9);
        assert_eq!(schedule("  E", 20).marks[0].start_ms, 0.0);
    }

    #[test]
    fn speed_marks_change_the_speed_for_the_rest_of_the_macro() {
        // The spec's example: ">TU >599 004 |SK|", +5 for TU and +10 for the rest.
        let s = schedule(">TU >599 004 |SK|", 20);
        let labels: Vec<&str> = s.marks.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(labels, ["T", "U", "5", "9", "9", "0", "0", "4", "SK"]);
        let t = &s.marks[0];
        assert!((t.end_ms - t.start_ms - 3.0 * 1200.0 / 25.0).abs() < 1e-9);
        let five = &s.marks[2];
        assert!((five.end_ms - five.start_ms - 9.0 * 1200.0 / 30.0).abs() < 1e-9);
        // SK is ...-.- as one character: 3 + 1 + 3 + 1 + 3 + 1 + ... with no letter gap.
        let sk = &s.marks[8];
        assert!((sk.end_ms - sk.start_ms - f64::from(units("...-.-")) * 40.0).abs() < 1e-9);
        // `<` never takes the speed to nothing.
        assert!(schedule("<<<<<<<<E", 20).total_ms.is_finite());
    }

    #[test]
    fn characters_with_no_code_take_no_time() {
        assert_eq!(schedule("E#E", 20).marks.len(), 2);
        assert_eq!(schedule("#$%", 20).total_ms, 0.0);
        assert_eq!(schedule("", 20), Schedule::default());
    }
}
