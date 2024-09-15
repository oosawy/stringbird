use clap::Parser as _;
use core::panic;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use swc::config::SourceMapsConfig;
use swc::{Compiler, PrintArgs};
use swc_common::comments::{Comment, CommentKind, Comments, SingleThreadedComments};
use swc_common::sync::Lrc;
use swc_common::{
    errors::{ColorConfig, Handler},
    SourceMap,
};
use swc_common::{FileName, SourceMapper, Span};
use swc_ecma_ast::{EsVersion, Expr, Lit, Tpl};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax};
use swc_ecma_parser::{PResult, TsSyntax};
use swc_ecma_visit::{VisitMut, VisitMutWith};

#[derive(Debug, clap::Parser)]
#[clap()]
struct Args {
    #[clap(subcommand)]
    subcommand: SubCommand,
}

#[derive(Debug, clap::Subcommand)]
enum SubCommand {
    Extract {
        #[clap(name = "files", required = true)]
        files: Vec<String>,
    },
    Apply {
        #[clap(name = "files", required = true)]
        files: Vec<String>,
    },
}

fn main() {
    let args = Args::parse();

    match args.subcommand {
        SubCommand::Extract { files } => extract(files),
        SubCommand::Apply { files } => apply(files),
    }
}

mod bird_format {
    use super::StringMap;
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader, Read, Result, Write},
    };

    fn encode_string(string: &str) -> String {
        let mut encoded = String::new();
        for c in string.chars() {
            match c {
                '\\' => encoded.push_str("\\\\"),
                '\n' => encoded.push_str("\\n"),
                _ => encoded.push(c),
            }
        }
        encoded
    }

    fn decode_string(string: &str) -> String {
        let mut decoded = String::new();
        let mut chars = string.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => decoded.push('\n'),
                    Some('\\') => decoded.push('\\'),
                    _ => decoded.push(c),
                }
            } else {
                decoded.push(c);
            }
        }
        decoded
    }

    pub fn parse<'a>(reader: impl Read) -> Result<StringMap> {
        let mut dict = HashMap::new();
        let reader = BufReader::new(reader);

        for line in reader.lines() {
            let line = line?;
            let mut parts = line.splitn(2, '=');
            let key = parts.next().unwrap();
            let value = parts.next().unwrap();
            println!("{}={}", key, value);
            dict.insert(key.to_string(), decode_string(value));
        }

        Ok(dict)
    }

    pub fn format(dict: StringMap, writer: &mut impl Write) -> Result<()> {
        for (k, v) in dict.iter() {
            writer.write(&format!("{}={}\n", k, encode_string(v)).as_bytes())?;
        }

        Ok(())
    }
}

fn extract(files: Vec<String>) {
    let mut dict = HashMap::new();

    for file in files {
        println!("Parsing {}", file);

        let map = pick_strings(&Path::new(&file)).expect("failed to parse file");
        dict.extend(map);
    }

    let mut output =
        BufWriter::new(File::create("stringbird").expect("failed to create output file"));

    bird_format::format(dict, &mut output).expect("failed to write output");

    println!("Output written to stringbird");
}

fn apply(files: Vec<String>) {
    let mut dict = HashMap::new();

    let input = BufReader::new(File::open("stringbird").expect("failed to open input file"));

    let map = bird_format::parse(input).expect("failed to parse input");
    dict.extend(map);

    for file in files {
        println!("Applying to {}", file);

        apply_strings(&Path::new(&file), dict.clone()).expect("failed to apply strings");
    }
}

// Represents a mapping of keys "#FOO" to string values "foo".
type StringMap = HashMap<String, String>;

fn parse_file(
    input: &Path,
) -> PResult<(
    swc_ecma_ast::Program,
    Lrc<SourceMap>,
    SingleThreadedComments,
)> {
    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(cm.clone()));

    let fm = cm
        .load_file(input)
        .unwrap_or_else(|e| panic!("failed to load {}: {}", input.display(), e));

    let comments = SingleThreadedComments::default();

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: true,
            ..Default::default()
        }),
        EsVersion::latest(),
        StringInput::from(&*fm),
        Some(&comments),
    );

    let mut parser = Parser::new_from(lexer);

    for e in parser.take_errors() {
        e.into_diagnostic(&handler).emit();
    }

    Ok((parser.parse_program()?, cm, comments))
}

fn parse_string(code: String, filename: &str) -> PResult<Box<Expr>> {
    let cm: Lrc<SourceMap> = Default::default();
    let handler = Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(cm.clone()));

    let fm = cm.new_source_file(FileName::Custom(filename.into()).into(), code);

    let comments = SingleThreadedComments::default();

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: true,
            ..Default::default()
        }),
        EsVersion::latest(),
        StringInput::from(&*fm),
        Some(&comments),
    );

    let mut parser = Parser::new_from(lexer);

    for e in parser.take_errors() {
        e.into_diagnostic(&handler).emit();
    }

    parser.parse_expr()
}

fn pick_strings(input: &Path) -> PResult<StringMap> {
    let (mut program, cm, comments) = parse_file(input)?;

    let mut strings = HashMap::new();

    let visitor = &mut StringBird {
        filename: input.file_name().unwrap().to_str().unwrap(),
        cm: &cm,
        comments: &comments,
        mode: BirdMode::Extract,
        strings: &mut strings,
    };

    program.visit_mut_with(visitor);

    Ok(strings)
}

fn apply_strings(input: &Path, strings: StringMap) -> PResult<()> {
    let (mut program, cm, comments) = parse_file(input)?;

    let mut strings = strings;

    let visitor = &mut StringBird {
        filename: input.file_name().unwrap().to_str().unwrap(),
        cm: &cm,
        comments: &comments,
        mode: BirdMode::Apply,
        strings: &mut strings,
    };

    program.visit_mut_with(visitor);

    let compiler = Compiler::new(cm);

    let result = compiler
        .print(
            &program,
            PrintArgs {
                comments: Some(&comments),
                source_map: SourceMapsConfig::Bool(false),
                ..Default::default()
            },
        )
        .expect("failed to print program");

    File::create(input)
        .expect("failed to create output file")
        .write_all(result.code.as_bytes())
        .expect("failed to write output");

    Ok(())
}

enum BirdMode {
    Extract,
    Apply,
}

struct StringBird<'a> {
    filename: &'a str,
    cm: &'a SourceMap,
    comments: &'a SingleThreadedComments,

    mode: BirdMode,
    strings: &'a mut StringMap,
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

impl VisitMut for StringBird<'_> {
    fn visit_mut_lit(&mut self, node: &mut Lit) {
        if let Lit::Str(s) = node {
            if let Some(c) = self.get_mark_comment(self.comments, s.span) {
                let key = &c.text[1..];
                let value = self.get_source_text(s.span);

                match self.mode {
                    BirdMode::Extract => {
                        self.strings.insert(key.to_string(), value);
                    }
                    BirdMode::Apply => {
                        if let Some(string) = self.strings.get(key) {
                            if value != *string {
                                match *parse_string(string.to_string(), self.filename).unwrap() {
                                    Expr::Lit(Lit::Str(mut parsed)) => {
                                        parsed.span = s.span;
                                        *s = parsed;
                                    }
                                    _ => panic!("expected string literal"),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn visit_mut_tpl(&mut self, node: &mut Tpl) {
        if let Some(c) = self.get_mark_comment(self.comments, node.span) {
            let key = &c.text[1..];
            let value = self.get_source_text(node.span);

            match self.mode {
                BirdMode::Extract => {
                    self.strings.insert(key.to_string(), value);
                }
                BirdMode::Apply => {
                    if let Some(string) = self.strings.get(key) {
                        if value != *string {
                            match *parse_string(string.to_string(), self.filename).unwrap() {
                                Expr::Tpl(mut parsed) => {
                                    parsed.span = node.span;
                                    *node = parsed;
                                }
                                _ => panic!("expected template literal"),
                            }
                        }
                    }
                }
            }
        }
    }
}
