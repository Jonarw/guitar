use winnow::{
    Result,
    ascii::{digit1, multispace1, till_line_ending},
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
        "ff".value(Accidental::DoubleFlat),
        "ss".value(Accidental::DoubleSharp),
        "f".value(Accidental::Flat),
        "s".value(Accidental::Sharp),
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
            '>'.value(Articulation::Accent),
        )),
    )
    .context(Label("Articulation"))
    .parse_next(input)
}

fn fingering(input: &mut &str) -> Result<u16> {
    preceded('-', digit1.parse_to())
        .context(Label("Fingering"))
        .parse_next(input)
}

fn tremolo(input: &mut &str) -> Result<Tremolo> {
    preceded(':', opt(digit1.parse_to()))
        .map(|d| Tremolo { repetition_duration: d })
        .context(Label("Tremolo"))
        .parse_next(input)
}

fn slur(input: &mut &str) -> Result<Slur> {
    preceded(opt('\\'), alt(('('.value(Slur::Start), ')'.value(Slur::End))))
        .context(Label("Slur"))
        .parse_next(input)
}

fn tie(input: &mut &str) -> Result<()> {
    '~'.void().parse_next(input)
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
        .map(|(ratio, augmentation)| NoteDuration {
            ratio,
            augmentation,
            tuplet: None,
        })
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

fn rest_multiplier(input: &mut &str) -> Result<u32> {
    preceded('*', digit1.parse_to()).parse_next(input)
}

fn rest_divider(input: &mut &str) -> Result<u32> {
    preceded('/', digit1.parse_to()).parse_next(input)
}

enum RestDurationModifier {
    Multiplier(u32),
    Divider(u32),
}

fn rest_duration_modifier(input: &mut &str) -> Result<RestDurationModifier> {
    alt((
        rest_multiplier.map(RestDurationModifier::Multiplier),
        rest_divider.map(RestDurationModifier::Divider),
    ))
    .parse_next(input)
}

fn rest_duration_modifiers(input: &mut &str) -> Result<(Vec<u32>, Vec<u32>)> {
    let mods: Vec<RestDurationModifier> = repeat(0.., rest_duration_modifier).parse_next(input)?;

    let mut multipliers = Vec::new();
    let mut dividers = Vec::new();

    for modifier in mods {
        match modifier {
            RestDurationModifier::Multiplier(m) => multipliers.push(m),
            RestDurationModifier::Divider(d) => dividers.push(d),
        }
    }

    Ok((multipliers, dividers))
}

fn rest_duration(input: &mut &str) -> Result<(NoteDuration, (Vec<u32>, Vec<u32>))> {
    (duration, rest_duration_modifiers).parse_next(input)
}

fn rest(input: &mut &str) -> Result<Rest> {
    preceded(alt(('r', 'R')), (opt(rest_duration), modifiers))
        .context(Label("Rest"))
        .parse_next(input)
        .map(|(rest_duration, (dynamic, articulation, crescendo, _, _, slur, _))| {
            let (duration, multipliers, dividers) = match rest_duration {
                Some((dur, (mul, div))) => (Some(dur), mul, div),
                None => (None, Vec::new(), Vec::new()),
            };

            Rest {
                duration,
                dynamic,
                articulation,
                crescendo,
                multipliers,
                dividers,
                slur,
            }
        })
}

enum Modifier {
    Dynamic(Dynamic),
    Articulation(Articulation),
    Crescendo(Crescendo),
    Tie,
    Fingering(u16),
    Slur(Slur),
    Tremolo(Tremolo),
}

fn modifier(input: &mut &str) -> Result<Modifier> {
    alt((
        dynamic.map(Modifier::Dynamic),
        articulation.map(Modifier::Articulation),
        crescendo.map(Modifier::Crescendo),
        tie.map(|()| Modifier::Tie),
        fingering.map(Modifier::Fingering),
        slur.map(Modifier::Slur),
        tremolo.map(Modifier::Tremolo),
    ))
    .parse_next(input)
}

fn modifiers(
    input: &mut &str,
) -> Result<(
    Option<Dynamic>,
    Articulation,
    Option<Crescendo>,
    bool,
    Option<u16>,
    Option<Slur>,
    Option<Tremolo>,
)> {
    let mods: Vec<Modifier> = repeat(0.., modifier).parse_next(input)?;

    let mut dynamic = None;
    let mut articulation = Articulation::none();
    let mut crescendo = None;
    let mut tie = false;
    let mut fingering = None;
    let mut slur = None;
    let mut tremolo = None;

    for m in mods {
        match m {
            Modifier::Dynamic(d) => dynamic = Some(d),
            Modifier::Articulation(a) => articulation |= a,
            Modifier::Crescendo(a) => crescendo = Some(a),
            Modifier::Tie => tie = true,
            Modifier::Fingering(f) => fingering = Some(f),
            Modifier::Slur(s) => slur = Some(s),
            Modifier::Tremolo(t) => tremolo = Some(t),
        }
    }

    Ok((dynamic, articulation, crescendo, tie, fingering, slur, tremolo))
}

fn note(input: &mut &str) -> Result<Note> {
    (pitch_class, accidental, octave, opt(duration), modifiers)
        .context(Label("Note"))
        .parse_next(input)
        .map(
            |(
                class,
                accidental,
                octave,
                duration,
                (dynamic, articulation, crescendo, tie, fingering, slur, tremolo),
            )| Note {
                class,
                accidental,
                octave,
                duration,
                dynamic,
                articulation,
                crescendo,
                tie,
                fingering,
                slur,
                tremolo,
            },
        )
}

fn event(input: &mut &str) -> Result<Event> {
    alt((
        rest.map(Event::Rest),
        note.map(Event::Note),
        tempo.map(Event::Tempo),
        time.map(Event::TimeSignature),
    ))
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
        alt((
            multispace1.void(),
            comment.void(),
            bar_line,
            expect_command("global").void(),
        )),
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
        accidental,
        _: discard,
        major_minor,
    )
    .context(Label("Key Signature"))
    .map(|(pitch_class, accidental, major_minor)| Key {
        tonic: pitch_class,
        accidental,
        major: major_minor,
    })
    .parse_next(input)
}

fn global(input: &mut &str) -> Result<Global> {
    variable("global", opt(key))
        .context(Label("'Global' variable"))
        .map(|key| Global { key })
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

fn language(input: &mut &str) -> Result<()> {
    seq!(
        discard,
        expect_command("language"),
        discard,
        delimited('"', "english", '"').context(Label("Language Name"))
    )
    .void()
    .context(Label("Language"))
    .parse_next(input)
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
    preceded(
        (discard, "title", discard, "=", discard),
        delimited('"', take_while(0.., |c: char| c != '"'), '"'),
    )
    .context(Label("Title"))
    .parse_next(input)
    .map(|s| s.to_owned())
}

pub fn header(input: &mut &str) -> Result<Header> {
    seq!(
        _: discard,
        _: expect_command("header").context(Label("Command")),
        _: discard,
        _: '{'.context(Label("Opening Brace")),
        _: discard,
        opt(title).context(Label("Title")),
        _: discard,
        _: '}'.context(Label("Closing Brace"))
    )
    .map(|(title,)| Header { title })
    .context(Label("Header"))
    .parse_next(input)
}

pub fn score(input: &mut &str) -> Result<LilyScore> {
    seq!(
        _: opt(version),
        _: discard,
        _: language,
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

/// Parses a full LilyPond score and preserves location/context on failure.
pub fn parse_score(input: &str) -> core::result::Result<LilyScore, winnow::error::ParseError<&str, ContextError>> {
    score.parse(input)
}

#[test]
fn parses_pitch_classes() {
    assert_eq!(pitch_class.parse("c"), Ok(PitchClass::C));
    assert_eq!(pitch_class.parse("g"), Ok(PitchClass::G));
}

#[test]
fn parses_accidentals() {
    assert_eq!(accidental.parse(""), Ok(Accidental::None));
    assert_eq!(accidental.parse("s"), Ok(Accidental::Sharp));
    assert_eq!(accidental.parse("ss"), Ok(Accidental::DoubleSharp));
    assert_eq!(accidental.parse("f"), Ok(Accidental::Flat));
    assert_eq!(accidental.parse("ff"), Ok(Accidental::DoubleFlat));
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
        Ok(NoteDuration {
            ratio: 4,
            augmentation: 0,
            tuplet: None,
        })
    );

    assert_eq!(
        duration.parse("16.."),
        Ok(NoteDuration {
            ratio: 16,
            augmentation: 2,
            tuplet: None,
        })
    );
}

#[test]
fn parses_note() {
    let note = note.parse("fs''8.\\mf\\<").unwrap();

    assert_eq!(note.class, PitchClass::F);
    assert_eq!(note.accidental, Accidental::Sharp);
    assert_eq!(note.octave, 2);
    assert_eq!(
        note.duration,
        Some(NoteDuration {
            ratio: 8,
            augmentation: 1,
            tuplet: None,
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
        Some(NoteDuration {
            ratio: 4,
            augmentation: 0,
            tuplet: None,
        })
    );
    assert_eq!(rest.dynamic, Some(Dynamic::P));
}

#[test]
fn parses_tempo() {
    assert_eq!(
        tempo.parse("\\tempo 4. = 120"),
        Ok(Tempo {
            note_duration: NoteDuration {
                ratio: 4,
                augmentation: 1,
                tuplet: None,
            },
            bpm: 120
        })
    );
}

#[test]
fn parses_key() {
    assert_eq!(
        key.parse("\\key gf \\major"),
        Ok(Key {
            tonic: PitchClass::G,
            accidental: Accidental::Flat,
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
        fs8\\f",
        )
        .unwrap();

    assert_eq!(events.len(), 3);
}

#[test]
fn parses_header() {
    let input = r#"
        \header
        %comment
        {
          title   =
          %comment
          "Smoke on the Water"
        }"#;

    let header = header.parse(input).unwrap();

    assert_eq!(header.title.unwrap(), "Smoke on the Water".to_owned());
}

#[test]
fn parses_global_section() {
    let input = r#"
        global = {
            \key bs \major
        }"#;

    let global = global.parse(input).unwrap();

    assert_eq!(
        global,
        Global {
            key: Some(Key {
                tonic: PitchClass::B,
                accidental: Accidental::Sharp,
                major: true,
            }),
        }
    );
}

#[test]
fn parses_full_score() {
    let input = r#"
        \version "2.26.0"
        \language "english"
        \header {
            title = "title"
        }

        global = {
            %comment
            \key g \major
        }

        stringOne
        %comment
        =
        {
            \tempo 4.. = 90
            \time 3/4
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
        score.global.key,
        Some(Key {
            tonic: PitchClass::G,
            accidental: Accidental::None,
            major: true,
        })
    );

    assert_eq!(score.parts.len(), 6);

    assert_eq!(score.parts[0].events.len(), 5);
    assert_eq!(score.parts[1].events.len(), 3);
    assert_eq!(score.parts[2].events.len(), 0);
    assert_eq!(score.parts[3].events.len(), 2);
    assert_eq!(score.parts[4].events.len(), 3);
    assert_eq!(score.parts[5].events.len(), 3);

    match &score.parts[0].events[0] {
        Event::Tempo(tempo) => {
            assert_eq!(
                *tempo,
                Tempo {
                    note_duration: NoteDuration {
                        ratio: 4,
                        augmentation: 2,
                        tuplet: None,
                    },
                    bpm: 90
                }
            );
        }
        _ => panic!("expected note"),
    }

    match &score.parts[0].events[1] {
        Event::TimeSignature(time_signature) => {
            assert_eq!(time_signature.numerator, 3);
            assert_eq!(time_signature.denominator, 4);
        }
        _ => panic!("expected note"),
    }

    match &score.parts[0].events[2] {
        Event::Note(note) => {
            assert_eq!(note.dynamic, Some(Dynamic::MF));
            assert_eq!(note.articulation, Articulation::Staccato);
            assert_eq!(note.crescendo, Some(Crescendo::CrescendoStart));
        }
        _ => panic!("expected note"),
    }

    match &score.parts[0].events[3] {
        Event::Note(note) => {
            assert_eq!(note.dynamic, Some(Dynamic::F));
            assert_eq!(note.articulation, Articulation::Portato);
            assert_eq!(note.crescendo, Some(Crescendo::DecrescendoStart));
        }
        _ => panic!("expected note"),
    }

    match &score.parts[4].events[0] {
        Event::Note(note) => {
            assert_eq!(note.class, PitchClass::E);
            assert_eq!(
                note.duration,
                Some(NoteDuration {
                    ratio: 4,
                    augmentation: 0,
                    tuplet: None,
                })
            );
            assert_eq!(note.dynamic, Some(Dynamic::MF));
        }
        _ => panic!("expected note"),
    }
}
