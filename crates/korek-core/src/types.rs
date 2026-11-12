//! Type expressions: `string`, `list<T>`, `map<string,T>`, `Name`.

use korek_ir::Type;

/// Parses a type expression. Whitespace around `<`, `,` and `>` is allowed.
///
/// # Errors
/// A message describing what is wrong.
pub fn parse_type(src: &str) -> Result<Type, String> {
    let mut p = P { s: src, i: 0 };
    let t = p.ty()?;
    p.ws();
    if p.i < src.len() {
        return Err(format!("unexpected `{}` in type `{src}`", &src[p.i..]));
    }
    Ok(t)
}

struct P<'a> {
    s: &'a str,
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.s[self.i..].starts_with(' ') {
            self.i += 1;
        }
    }

    fn ident(&mut self) -> &str {
        self.ws();
        let start = self.i;
        while self.s[self.i..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.i += 1;
        }
        &self.s[start..self.i]
    }

    fn eat(&mut self, c: char) -> Result<(), String> {
        self.ws();
        if self.s[self.i..].starts_with(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{c}` in type `{}`", self.s))
        }
    }

    fn ty(&mut self) -> Result<Type, String> {
        let name = self.ident().to_owned();
        Ok(match name.as_str() {
            "" => return Err(format!("expected a type name in `{}`", self.s)),
            "list" => {
                self.eat('<')?;
                let t = self.ty()?;
                self.eat('>')?;
                Type::List(Box::new(t))
            }
            "map" => {
                self.eat('<')?;
                let k = self.ident().to_owned();
                if k != "string" {
                    return Err(format!(
                        "map keys must be `string`, found `{k}` in `{}`",
                        self.s
                    ));
                }
                self.eat(',')?;
                let t = self.ty()?;
                self.eat('>')?;
                Type::Map(Box::new(t))
            }
            other => Type::primitive(other).unwrap_or(Type::Named(name)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested() {
        assert_eq!(
            parse_type("map<string, list<Foo>>").unwrap(),
            Type::Map(Box::new(Type::List(Box::new(Type::Named("Foo".into())))))
        );
        assert!(parse_type("map<i64,string>").is_err());
        assert!(parse_type("list<string").is_err());
        assert!(parse_type("list<string>>").is_err());
    }
}
