use winnow::{
    Result,
    ascii::{alpha1, digit1, multispace1, till_line_ending},
    combinator::{alt, delimited, opt, preceded, repeat, seq, terminated},
    error::{ContextError, StrContext::Label},
    prelude::*,
    token::take_while,
};

use crate::syntax::ast::*;

fn pitch_class(input: &mut &str) -> Result<PitchClass> {
    alt((
        'a'.value(PitchClass::A),
        'b'.value(PitchClass::B),
        'c'.value(PitchClass::C),
        'd'.value(PitchClass::D),
        'e'.value(PitchClass::E),
        'f'.value(PitchClass::F),
        'g'.value(PitchClass::G),
    ))
    .context(Label("Pitch"))
    .parse_next(input)
}

fn accidental(input: &mut &str) -> Result<Accidental> {
    alt((
        "eses".value(Accidental::DoubleFlat),
        "isis".value(Accidental::DoubleSharp),
        "es".value(Accidental::Flat),
        "is".value(Accidental::Sharp),
        "".value(Accidental::None),
    ))
    .context(Label("Pitch"))
    .parse_next(input)
}

fn articulation(input: &mut &str) -> Result<Articulation> {
    preceded(
        '-',
        alt((
            '.'.value(Articulation::Staccato),
            '!'.value(Articulation::Staccatissimo),
            '-'.value(Articulation::Tenuto),
            '_'.value(Articulation::Portato),
            '^'.value(Articulation::Marcato),
        )),
    )
    .context(Label("Articulation"))
    .parse_next(input)
}

fn octave(input: &mut &str) -> Result<i8> {
    repeat(0.., alt(('\''.value(1), ','.value(-1))))
        .fold(|| 0, |acc, item| acc + item)
        .context(Label("Octave"))
        .parse_next(input)
}

fn duration(input: &mut &str) -> Result<NoteDuration> {
    (digit1.parse_to(), repeat(0.., '.').fold(|| 0, |i, _| i + 1))
        .context(Label("Duration"))
        .map(|(ratio, augmentation)| NoteDuration { ratio, augmentation })
        .parse_next(input)
}

fn command<'a, O, P>(parser: P) -> impl Parser<&'a str, O, ContextError>
where
    P: Parser<&'a str, O, ContextError>,
{
    preceded('\\', parser).context(Label("Command"))
}

fn expect_command<'s>(command_name: &'static str) -> impl Parser<&'s str, (), ContextError> {
    command(command_name).void()
}

fn dynamic(input: &mut &str) -> Result<Dynamic> {
    command(alt((
        "ppp".value(Dynamic::PPP),
        "fff".value(Dynamic::FFF),
        "pp".value(Dynamic::PP),
        "ff".value(Dynamic::FF),
        "p".value(Dynamic::P),
        "f".value(Dynamic::F),
        "mp".value(Dynamic::MP),
        "mf".value(Dynamic::MF),
    )))
    .context(Label("Dynamic"))
    .parse_next(input)
}

fn crescendo(input: &mut &str) -> Result<Crescendo> {
    command(alt((
        "<".value(Crescendo::CrescendoStart),
        ">".value(Crescendo::DecrescendoStart),
        "!".value(Crescendo::End),
    )))
    .context(Label("Crescendo"))
    .parse_next(input)
}

fn rest(input: &mut &str) -> Result<Rest> {
    preceded('r', (opt(duration), opt(dynamic)))
        .context(Label("Rest"))
        .parse_next(input)
        .map(|(duration, dynamic)| Rest { duration, dynamic })
}

enum Modifier {
    Dynamic(Dynamic),
    Articulation(Articulation),
    Crescendo(Crescendo),
}

fn modifier(input: &mut &str) -> Result<Modifier> {
    alt((
        dynamic.map(Modifier::Dynamic),
        articulation.map(Modifier::Articulation),
        crescendo.map(Modifier::Crescendo),
    ))
    .parse_next(input)
}

fn modifiers(input: &mut &str) -> Result<(Option<Dynamic>, Option<Articulation>, Option<Crescendo>)> {
    let mods: Vec<Modifier> = repeat(0.., modifier).parse_next(input)?;

    let mut dynamic = None;
    let mut articulation = None;
    let mut crescendo = None;

    for m in mods {
        match m {
            Modifier::Dynamic(d) => dynamic = Some(d),
            Modifier::Articulation(a) => articulation = Some(a),
            Modifier::Crescendo(a) => crescendo = Some(a),
        }
    }

    Ok((dynamic, articulation, crescendo))
}

fn note(input: &mut &str) -> Result<Note> {
    (pitch_class, accidental, octave, opt(duration), modifiers)
        .context(Label("Note"))
        .parse_next(input)
        .map(
            |(class, accidental, octave, duration, (dynamic, articulation, crescendo))| Note {
                class,
                accidental,
                octave,
                duration,
                dynamic,
                articulation,
                crescendo,
            },
        )
}

fn event(input: &mut &str) -> Result<Event> {
    alt((rest.map(Event::Rest), note.map(Event::Note)))
        .context(Label("Event"))
        .parse_next(input)
}

fn comment<'s>(input: &mut &'s str) -> Result<&'s str> {
    preceded('%', till_line_ending)
        .context(Label("Comment"))
        .parse_next(input)
}

fn bar_line(input: &mut &str) -> Result<()> {
    '|'.value(()).context(Label("Barline")).parse_next(input)
}

fn discard(input: &mut &str) -> Result<()> {
    repeat(0.., alt((multispace1.void(), comment.void())))
        .context(Label("Discard"))
        .parse_next(input)
}

fn part_discard(input: &mut &str) -> Result<()> {
    repeat(
        0..,
        alt((multispace1.void(), comment.void(), bar_line, command(alpha1).void())),
    )
    .context(Label("Discard (note)"))
    .parse_next(input)
}

fn lexeme(input: &mut &str) -> Result<Event> {
    preceded(part_discard, event).context(Label("Lexeme")).parse_next(input)
}

fn events(input: &mut &str) -> Result<Vec<Event>> {
    terminated(repeat(0.., lexeme), part_discard)
        .context(Label("Events"))
        .parse_next(input)
}

fn variable<'a, P, O>(name: &'static str, body: P) -> impl Parser<&'a str, O, ContextError>
where
    P: Parser<&'a str, O, ContextError>,
{
    delimited(
        (discard, name, discard, "=", discard, "{").context(Label("Prefix")),
        body.context(Label("Body")),
        (discard, "}").context(Label("Suffix")),
    )
    .context(Label("Variable"))
}

fn string_part<'a>(name: &'static str) -> impl Parser<&'a str, LilyPart, ContextError> {
    variable(name, events).context(Label(name)).map(|events| LilyPart {
        events,
        name: name.to_string(),
    })
}

fn tempo(input: &mut &str) -> Result<Tempo> {
    seq!(
        _: (discard, expect_command("tempo"), discard),
        duration,
        _: (discard, "=", discard),
        digit1.parse_to(),
    )
    .map(|(duration, bpm)| Tempo {
        note_duration: duration,
        bpm,
    })
    .context(Label("Tempo"))
    .parse_next(input)
}

fn time(input: &mut &str) -> Result<TimeSignature> {
    seq!(
        _: discard,
        _: expect_command("time"),
        _: discard,
        digit1.parse_to(),
        _: '/',
        digit1.parse_to(),
    )
    .context(Label("Time Signature"))
    .map(|(num, den)| TimeSignature {
        numerator: num,
        denominator: den,
    })
    .parse_next(input)
}

fn major_minor(input: &mut &str) -> Result<bool> {
    alt((
        expect_command("major").map(|_| true),
        expect_command("minor").map(|_| false),
    ))
    .context(Label("Key Class"))
    .parse_next(input)
}

fn key(input: &mut &str) -> Result<Key> {
    seq!(
        _: discard,
        _: expect_command("key"),
        _: discard,
        pitch_class,
        _: discard,
        major_minor,
    )
    .context(Label("Key Signature"))
    .map(|(pitch_class, major_minor)| Key {
        tonic: pitch_class,
        major: major_minor,
    })
    .parse_next(input)
}

fn global(input: &mut &str) -> Result<Global> {
    variable("global", (opt(tempo), opt(time), opt(key)))
        .context(Label("'Global' variable"))
        .map(|(tempo, time, key)| Global { tempo, time, key })
        .parse_next(input)
}

fn strings(input: &mut &str) -> Result<[LilyPart; NUMBER_OF_STRINGS]> {
    Ok([
        string_part("stringOne").parse_next(input)?,
        string_part("stringTwo").parse_next(input)?,
        string_part("stringThree").parse_next(input)?,
        string_part("stringFour").parse_next(input)?,
        string_part("stringFive").parse_next(input)?,
        string_part("stringSix").parse_next(input)?,
    ])
}

fn version(input: &mut &str) -> Result<()> {
    seq!(
        discard,
        expect_command("version"),
        discard,
        delimited('"', take_while(1.., |c: char| c.is_numeric() || c == '.'), '"').context(Label("Version Number"))
    )
    .void()
    .context(Label("Version"))
    .parse_next(input)
}

pub fn title(input: &mut &str) -> Result<String> {
    preceded((discard, "title", discard, "=", discard), delimited('"', alpha1, '"'))
        .context(Label("Title"))
        .parse_next(input)
        .map(|s| s.to_owned())
}

pub fn header(input: &mut &str) -> Result<Header> {
    variable("header", opt(title))
        .map(|title| Header { title })
        .context(Label("Header"))
        .parse_next(input)
}

pub fn score(input: &mut &str) -> Result<LilyScore> {
    seq!(
        _: opt(version),
        _: discard,
        opt(header),
        _: discard,
        global,
        strings,
        _: winnow::token::rest,
    )
    .context(Label("Score"))
    .map(|(header, global, strings)| LilyScore {
        header,
        global,
        parts: strings,
    })
    .parse_next(input)
}

#[test]
fn parses_pitch_classes() {
    assert_eq!(pitch_class.parse("c"), Ok(PitchClass::C));
    assert_eq!(pitch_class.parse("g"), Ok(PitchClass::G));
}

#[test]
fn parses_accidentals() {
    assert_eq!(accidental.parse(""), Ok(Accidental::None));
    assert_eq!(accidental.parse("is"), Ok(Accidental::Sharp));
    assert_eq!(accidental.parse("isis"), Ok(Accidental::DoubleSharp));
    assert_eq!(accidental.parse("es"), Ok(Accidental::Flat));
    assert_eq!(accidental.parse("eses"), Ok(Accidental::DoubleFlat));
}

#[test]
fn parses_octaves() {
    assert_eq!(octave.parse(""), Ok(0));
    assert_eq!(octave.parse("'"), Ok(1));
    assert_eq!(octave.parse("''"), Ok(2));
    assert_eq!(octave.parse(","), Ok(-1));
    assert_eq!(octave.parse(",,,"), Ok(-3));
}

#[test]
fn parses_duration() {
    assert_eq!(
        duration.parse("4"),
        Ok(Duration {
            ratio: 4,
            augmentation: 0
        })
    );

    assert_eq!(
        duration.parse("16.."),
        Ok(Duration {
            ratio: 16,
            augmentation: 2
        })
    );
}

#[test]
fn parses_note() {
    let note = note.parse("fis''8.\\mf\\<").unwrap();

    assert_eq!(note.class, PitchClass::F);
    assert_eq!(note.accidental, Accidental::Sharp);
    assert_eq!(note.octave, 2);
    assert_eq!(
        note.duration,
        Some(Duration {
            ratio: 8,
            augmentation: 1
        })
    );
    assert_eq!(note.dynamic, Some(Dynamic::MF));
    assert_eq!(note.crescendo, Some(Crescendo::CrescendoStart));
}

#[test]
fn parses_rest() {
    let rest = rest.parse("r4\\p").unwrap();

    assert_eq!(
        rest.duration,
        Some(Duration {
            ratio: 4,
            augmentation: 0
        })
    );
    assert_eq!(rest.dynamic, Some(Dynamic::P));
}

#[test]
fn parses_tempo() {
    assert_eq!(
        tempo.parse("\\tempo 4. = 120"),
        Ok(Tempo {
            note_duration: Duration {
                ratio: 4,
                augmentation: 1
            },
            bpm: 120
        })
    );
}

#[test]
fn parses_key() {
    assert_eq!(
        key.parse("\\key g \\major"),
        Ok(Key {
            tonic: PitchClass::G,
            major: true,
        })
    );
}

#[test]
fn parses_events() {
    let events = events
        .parse(
            "c4
        r4
        fis8\\f",
        )
        .unwrap();

    assert_eq!(events.len(), 3);
}

#[test]
fn parses_global_section() {
    let input = r#"
        global = {
            \tempo 4 = 120
            \time 4/4
            \key c \major
        }"#;

    let global = global.parse(input).unwrap();

    assert_eq!(
        global,
        Global {
            tempo: Some(Tempo {
                note_duration: Duration {
                    ratio: 4,
                    augmentation: 0
                },
                bpm: 120
            }),
            time: Some(TimeSignature {
                numerator: 4,
                denominator: 4,
            }),
            key: Some(Key {
                tonic: PitchClass::C,
                major: true,
            }),
        }
    );
}

#[test]
fn parses_full_score() {
    let input = r#"
        \version "2.26.0"

        header = {
            title = "title"
        }

        global = {
            %comment
            \tempo 4.. = 90
            \time 3/4
            \key g \major
        }

        stringOne
        %comment
        =
        {
            c4\mf-.\< d4\>-_\f e4 |
        }

        stringTwo = {
            r4 | g4 a4 |
        }

        stringThree = {
        }

        stringFour = {
            b,2 %comment
            c4 |
        }

        stringFive = {
            e4\mf r4 d4 |
        }

        stringSix = {
            g,4 g,4 g,4 |
        }

        The rest of the file is ignored, no matter what it is.
        "#;

    let score = score.parse(input).unwrap();

    assert_eq!(score.header.unwrap().title.unwrap(), "title");
    assert_eq!(
        score.global.tempo,
        Some(Tempo {
            note_duration: Duration {
                ratio: 4,
                augmentation: 2
            },
            bpm: 90
        })
    );

    assert_eq!(
        score.global.time,
        Some(TimeSignature {
            numerator: 3,
            denominator: 4,
        })
    );

    assert_eq!(
        score.global.key,
        Some(Key {
            tonic: PitchClass::G,
            major: true,
        })
    );

    assert_eq!(score.parts.len(), 6);

    assert_eq!(score.parts[0].events.len(), 3);
    assert_eq!(score.parts[1].events.len(), 3);
    assert_eq!(score.parts[2].events.len(), 0);
    assert_eq!(score.parts[3].events.len(), 2);
    assert_eq!(score.parts[4].events.len(), 3);
    assert_eq!(score.parts[5].events.len(), 3);

    match &score.parts[0].events[0] {
        Event::Note(note) => {
            assert_eq!(note.dynamic, Some(Dynamic::MF));
            assert_eq!(note.articulation, Some(Articulation::Staccato));
            assert_eq!(note.crescendo, Some(Crescendo::CrescendoStart));
        }
        _ => panic!("expected note"),
    }

    match &score.parts[0].events[1] {
        Event::Note(note) => {
            assert_eq!(note.dynamic, Some(Dynamic::F));
            assert_eq!(note.articulation, Some(Articulation::Portato));
            assert_eq!(note.crescendo, Some(Crescendo::DecrescendoStart));
        }
        _ => panic!("expected note"),
    }

    match &score.parts[4].events[0] {
        Event::Note(note) => {
            assert_eq!(note.class, PitchClass::E);
            assert_eq!(
                note.duration,
                Some(Duration {
                    ratio: 4,
                    augmentation: 0
                })
            );
            assert_eq!(note.dynamic, Some(Dynamic::MF));
        }
        _ => panic!("expected note"),
    }
}
