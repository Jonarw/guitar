use winnow::{
    Result,
    ascii::{digit1, multispace1, till_line_ending},
    combinator::{alt, delimited, opt, preceded, repeat, seq, terminated},
    error::{
        ContextError,
        StrContext::{self, Expected, Label},
        StrContextValue,
    },
    prelude::*,
    token::{any, take_while},
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
    .context(Expected(StrContextValue::Description("letter between 'a' and 'g'")))
    .context(Label("Pitch"))
    .parse_next(input)
}

fn accidental(input: &mut &str) -> Result<Accidental> {
    alt((
        "eses".value(Accidental::Minus2),
        "isis".value(Accidental::Plus2),
        "es".value(Accidental::Minus1),
        "is".value(Accidental::Plus1),
        "".value(Accidental::Zero),
    ))
    .context(Expected(StrContextValue::Description(
        "accidental suffix (es, is, eses, isis)",
    )))
    .context(Label("Pitch"))
    .parse_next(input)
}

fn octave(input: &mut &str) -> Result<i8> {
    repeat(0.., alt(('\''.value(1), ','.value(-1))))
        .fold(|| 0, |acc, item| acc + item)
        .context(Label("Octave"))
        .parse_next(input)
}

fn duration(input: &mut &str) -> Result<Option<u32>> {
    opt(digit1.parse_to()).context(Label("Duration")).parse_next(input)
}

fn command<'s>(input: &mut &'s str) -> Result<&'s str> {
    preceded('\\', take_while(1.., |c: char| c.is_alpha() || c == '<' || c == '>'))
        .context(Label("Command"))
        .context(Expected(StrContextValue::Description("\\[Command]")))
        .parse_next(input)
}

fn expect_command<'s>(command_name: &'static str) -> impl Parser<&'s str, (), ContextError> {
    command
        .verify(move |cmd: &str| cmd == command_name)
        .context(Label(command_name))
        .void()
}

impl TryFrom<&str> for Dynamic {
    type Error = ContextError;

    fn try_from(cmd: &str) -> Result<Self, Self::Error> {
        match cmd {
            "ppp" => Ok(Dynamic::PPP),
            "fff" => Ok(Dynamic::FFF),
            "pp" => Ok(Dynamic::PP),
            "ff" => Ok(Dynamic::FF),
            "p" => Ok(Dynamic::P),
            "f" => Ok(Dynamic::F),
            "mp" => Ok(Dynamic::MP),
            "mf" => Ok(Dynamic::MF),
            ">" => Ok(Dynamic::DecrescendoStart),
            "<" => Ok(Dynamic::CrescendoStart),
            "!" => Ok(Dynamic::CrescendoEnd),
            _ => {
                let mut error = ContextError::new();
                error.push(Label("Dynamic"));
                Err(error)
            }
        }
    }
}

fn dynamic(input: &mut &str) -> Result<Option<Dynamic>> {
    let Some(cmd) = opt(command).context(Label("Dynamic command")).parse_next(input)? else {
        return Ok(None);
    };

    Ok(Some(Dynamic::try_from(cmd)?))
}

fn rest(input: &mut &str) -> Result<Rest> {
    preceded('r', (duration, dynamic))
        .context(Label("Rest"))
        .parse_next(input)
        .map(|(duration, dynamic)| Rest { duration, dynamic })
}

fn note(input: &mut &str) -> Result<Note> {
    (pitch_class, accidental, octave, duration, dynamic)
        .context(Label("Note"))
        .parse_next(input)
        .map(|(class, accidental, octave, duration, dynamic)| Note {
            class,
            accidental,
            octave,
            duration,
            dynamic,
        })
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
    repeat(0.., alt((multispace1.void(), comment.void(), bar_line, command.void())))
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

fn string_part<'a>(name: &'static str) -> impl Parser<&'a str, StringPart, ContextError> {
    variable(name, events).context(Label(name)).map(|events| StringPart {
        events,
        name: name.to_string(),
    })
}

fn tempo(input: &mut &str) -> Result<u32> {
    preceded(
        (discard, expect_command("tempo"), discard, "4", discard, "=", discard),
        digit1.parse_to(),
    )
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

fn strings(input: &mut &str) -> Result<[StringPart; NUMBER_OF_STRINGS]> {
    Ok([
        string_part("stringOne").parse_next(input)?,
        string_part("stringTwo").parse_next(input)?,
        string_part("stringThree").parse_next(input)?,
        string_part("stringFour").parse_next(input)?,
        string_part("stringFive").parse_next(input)?,
        string_part("stringSix").parse_next(input)?,
    ])
}

pub fn score(input: &mut &str) -> Result<Score> {
    seq!(
        _: discard,
        global,
        strings,
        _: winnow::token::rest,
    )
    .context(Label("Score"))
    .map(|(global, strings)| Score { global, strings })
    .parse_next(input)
}

#[test]
fn parses_pitch_classes() {
    assert_eq!(pitch_class.parse("c"), Ok(PitchClass::C));
    assert_eq!(pitch_class.parse("g"), Ok(PitchClass::G));
}

#[test]
fn parses_accidentals() {
    assert_eq!(accidental.parse(""), Ok(Accidental::Zero));
    assert_eq!(accidental.parse("is"), Ok(Accidental::Plus1));
    assert_eq!(accidental.parse("isis"), Ok(Accidental::Plus2));
    assert_eq!(accidental.parse("es"), Ok(Accidental::Minus1));
    assert_eq!(accidental.parse("eses"), Ok(Accidental::Minus2));
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
    assert_eq!(duration.parse("4"), Ok(Some(4)));
    assert_eq!(duration.parse("16"), Ok(Some(16)));
    assert_eq!(duration.parse(""), Ok(None));
}

#[test]
fn parses_note() {
    let note = note.parse("fis''8\\mf").unwrap();

    assert_eq!(note.class, PitchClass::F);
    assert_eq!(note.accidental, Accidental::Plus1);
    assert_eq!(note.octave, 2);
    assert_eq!(note.duration, Some(8));
    assert_eq!(note.dynamic, Some(Dynamic::MF));
}

#[test]
fn parses_rest() {
    let rest = rest.parse("r4\\p").unwrap();

    assert_eq!(rest.duration, Some(4));
    assert_eq!(rest.dynamic, Some(Dynamic::P));
}

#[test]
fn parses_tempo() {
    assert_eq!(tempo.parse("\\tempo 4 = 120"), Ok(120));
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
            tempo: Some(120),
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
        global = {
            %comment
            \tempo 4 = 90
            \time 3/4
            \key g \major
        }

        stringOne
        %comment
        =
        {
            c4 d4 e4 |
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

    assert_eq!(score.global.tempo, Some(90));

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

    assert_eq!(score.strings.len(), 6);

    assert_eq!(score.strings[0].events.len(), 3);
    assert_eq!(score.strings[1].events.len(), 3);
    assert_eq!(score.strings[2].events.len(), 0);
    assert_eq!(score.strings[3].events.len(), 2);
    assert_eq!(score.strings[4].events.len(), 3);
    assert_eq!(score.strings[5].events.len(), 3);

    match &score.strings[4].events[0] {
        Event::Note(note) => {
            assert_eq!(note.class, PitchClass::E);
            assert_eq!(note.duration, Some(4));
            assert_eq!(note.dynamic, Some(Dynamic::MF));
        }
        _ => panic!("expected note"),
    }
}
