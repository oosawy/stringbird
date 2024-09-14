use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use swc_common::comments::{Comment, CommentKind, Comments, SingleThreadedComments};
use swc_common::sync::Lrc;
use swc_common::{
    errors::{ColorConfig, Handler},
    SourceMap,
};
use swc_common::{SourceMapper, Span};
use swc_ecma_ast::Lit;
use swc_ecma_parser::PResult;
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecma_visit::{Visit, VisitWith};

// Represents a mapping of keys "#123" to string values "foo".
type StringMap = HashMap<String, String>;

fn main() {
    let inputs = std::env::args().skip(1).collect::<Vec<_>>();

    let mut dict = HashMap::new();

    for input in inputs {
        println!("Parsing {}", input);

        match parse(&Path::new(&input)) {
            Ok(map) => dict.extend(map),
            Err(e) => eprintln!("Error: {:?}", e),
        }
    }

    let mut output =
        BufWriter::new(File::create("stringbird").expect("failed to create output file"));

    for (k, v) in dict.iter() {
        output.write(format!("{}={}\n", k, v).as_bytes()).unwrap();
    }

    println!("Output written to stringbird");
}

fn parse(input: &Path) -> PResult<StringMap> {
    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(cm.clone()));

    let fm = cm
        .load_file(input)
        .unwrap_or_else(|e| panic!("failed to load {}: {}", input.display(), e));

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

    let mut strings = HashMap::new();

    let visitor = &mut StringBird {
        cm: &cm,
        comments: &comments,
        strings: &mut strings,
    };

    parser.parse_program()?.visit_with(visitor);

    Ok(strings)
}

struct StringBird<'a> {
    cm: &'a SourceMap,
    comments: &'a SingleThreadedComments,

    strings: &'a mut HashMap<String, String>,
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
                    .insert(c.text[1..].to_string(), self.get_source_text(s.span));
            }
        }
    }

    fn visit_tpl(&mut self, node: &swc_ecma_ast::Tpl) {
        if let Some(c) = self.get_mark_comment(self.comments, node.span) {
            self.strings
                .insert(c.text[1..].to_string(), self.get_source_text(node.span));
        }
    }
}
