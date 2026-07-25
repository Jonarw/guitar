use std::todo;

use lilyparse::syntax::ast::{Dynamic, LilyPart};

use crate::machine_score::timing::Notes;

struct CrescendoBlock {
    start: Notes,
    end: Notes,
    start_dynamic: Dynamic,
    end_dynamic: Dynamic,
}

pub struct DynamicHelper {
    crescendo_blocks: Vec<CrescendoBlock>,
}

impl DynamicHelper {
    pub fn new(part: &LilyPart) -> Self {
        for event in &part.events {}

        todo!()
    }
}
