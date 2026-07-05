use crate::model::ParsedModule;

pub fn parse_module(_text: &str) -> ParsedModule {
    ParsedModule {
        ops: Vec::new(),
        functions: Vec::new(),
    }
}
