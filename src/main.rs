use std::collections::HashMap;
use swc_common::comments::{Comment, CommentKind, Comments, SingleThreadedComments};
use swc_common::sync::Lrc;
use swc_common::{
    errors::{ColorConfig, Handler},
    FileName, SourceMap,
};
use swc_common::{SourceMapper, Span};
use swc_ecma_ast::Lit;
use swc_ecma_parser::PResult;
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecma_visit::{Visit, VisitWith};

fn main() {
    // let input = std::env::args().nth(1).expect("missing input file");

    let _ = parse();
}

fn parse() -> PResult<()> {
    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(cm.clone()));

    // let fm = cm
    //     .load_file(Path::new("test.js"))
    //     .expect("failed to load test.js");
    let fm = cm.new_source_file(
        FileName::Custom("test.js".into()).into(),
        "function foo(world = /*#123*/\"world\") {return /*#456*/`hello ${world}`}".into(),
    );

    let comments = SingleThreadedComments::default();

    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        Default::default(),
        StringInput::from(&*fm),
        Some(&comments),
    );

    let mut parser = Parser::new_from(lexer);

    for e in parser.take_errors() {
        e.into_diagnostic(&handler).emit();
    }

    let strings = HashMap::new();

    let visitor = &mut StringBird {
        cm: &cm,
        comments: &comments,
        strings,
    };

    parser.parse_program()?.visit_with(visitor);

    for (k, v) in visitor.strings.iter() {
        println!("{}: {}", k, v);
    }

    Ok(())
}

struct StringBird<'a> {
    cm: &'a SourceMap,
    comments: &'a SingleThreadedComments,

    strings: HashMap<String, String>,
}

impl StringBird<'_> {
    fn get_source_text(&self, span: Span) -> String {
        self.cm.span_to_snippet(span).unwrap()
    }

    fn get_mark_comment<'a>(
        &self,
        comments: &'a SingleThreadedComments,
        span: Span,
    ) -> Option<Comment> {
        comments
            .get_leading(span.lo())
            .and_then(|c| c.last().cloned())
            .filter(|c| c.kind == CommentKind::Block && c.text.starts_with("#"))
    }
}

impl Visit for StringBird<'_> {
    fn visit_lit(&mut self, node: &Lit) {
        if let Lit::Str(s) = node {
            if let Some(c) = self.get_mark_comment(self.comments, s.span) {
                self.strings
                    .insert(c.text.to_string(), self.get_source_text(s.span));
            }
        }
    }

    fn visit_tpl(&mut self, node: &swc_ecma_ast::Tpl) {
        if let Some(c) = self.get_mark_comment(self.comments, node.span) {
            self.strings
                .insert(c.text.to_string(), self.get_source_text(node.span));
        }
    }
}
