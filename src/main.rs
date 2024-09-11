use std::collections::HashMap;
use swc_common::comments::{CommentKind, Comments, SingleThreadedComments};
use swc_common::sync::Lrc;
use swc_common::Spanned;
use swc_common::{
    errors::{ColorConfig, Handler},
    FileName, SourceMap,
};
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
        "function foo(s = /*#123*/\"default\") {return /*#456*/'hello'}".into(),
    );

    let comments = SingleThreadedComments::default();

    let lexer = Lexer::new(
        // We want to parse ecmascript
        Syntax::Es(Default::default()),
        // EsVersion defaults to es5
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
    comments: &'a SingleThreadedComments,

    strings: HashMap<String, String>,
}

impl Visit for StringBird<'_> {
    fn visit_lit(&mut self, node: &Lit) {
        if let Lit::Str(s) = node {
            if let Some(c) = self
                .comments
                .get_leading(s.span_lo())
                .as_ref()
                .and_then(|c| c.last())
            {
                if c.kind == CommentKind::Block && c.text.starts_with("#") {
                    self.strings.insert(c.text.to_string(), s.value.to_string());
                }
            }
        }
    }
}
