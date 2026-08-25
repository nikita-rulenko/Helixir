use crate::helixc::parser::location::Loc;

#[derive(Debug, Clone)]
pub struct Fix {
    pub span: Option<Loc>,
    pub to_remove: Option<Loc>,
    pub to_add: Option<String>,
}

impl Fix {
    pub fn new(span: Option<Loc>, to_remove: Option<Loc>, to_add: Option<String>) -> Self {
        Self {
            span,
            to_remove,
            to_add,
        }
    }
}
