use winnow::{
    Result,
    ascii::{alpha1, digit1, multispace0, multispace1, till_line_ending},
    combinator::{Repeat, alt, delimited, opt, preceded, repeat},
    error::ContextError,
    prelude::*,
    token::any,
};

use crate::syntax::ast::{Accidental, Dynamic, Event, NUMBER_OF_STRINGS, Note, PitchClass, Rest, StringPart};

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
    .parse_next(input)
}

fn octave(input: &mut &str) -> Result<i8> {
    repeat(0.., alt(('\''.value(1), ','.value(-1))))
        .fold(|| 0, |acc, item| acc + item)
        .parse_next(input)
}

fn duration(input: &mut &str) -> Result<Option<u32>> {
    opt(digit1.parse_to()).parse_next(input)
}

fn command<'s>(input: &mut &'s str) -> Result<&'s str> {
    preceded('\\', alpha1).parse_next(input)
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
            _ => Err(ContextError::new()),
        }
    }
}

fn dynamic(input: &mut &str) -> Result<Option<Dynamic>> {
    let Some(cmd) = opt(command).parse_next(input)? else {
        return Ok(None);
    };

    Ok(Some(Dynamic::try_from(cmd)?))
}

fn rest(input: &mut &str) -> Result<Rest> {
    preceded('r', (duration, dynamic))
        .parse_next(input)
        .map(|(duration, dynamic)| Rest { duration, dynamic })
}

fn note(input: &mut &str) -> Result<Note> {
    (pitch_class, accidental, octave, duration, dynamic)
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
    alt((rest.map(Event::Rest), note.map(Event::Note))).parse_next(input)
}

fn comment<'s>(input: &mut &'s str) -> Result<&'s str> {
    preceded('%', till_line_ending).parse_next(input)
}

fn bar_line(input: &mut &str) -> Result<()> {
    '|'.value(()).parse_next(input)
}

fn general_discard(input: &mut &str) -> Result<()> {
    repeat(0.., alt((multispace1.void(), comment.void()))).parse_next(input)
}

fn part_discard(input: &mut &str) -> Result<()> {
    repeat(0.., alt((general_discard, bar_line, command.void()))).parse_next(input)
}

fn lexeme(input: &mut &str) -> Result<Event> {
    preceded(part_discard, event).parse_next(input)
}

fn events(input: &mut &str) -> Result<Vec<Event>> {
    repeat(0.., lexeme).parse_next(input)
}

fn string_part<'a>(name: &'static str) -> impl Parser<&'a str, StringPart, ContextError> {
    delimited(
        (general_discard, name, general_discard, "=", general_discard, "{"),
        events.map(|events| StringPart {
            events,
            name: name.to_string(),
        }),
        (general_discard, "}"),
    )
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
