//! Numeric expressions for the CLI and shell editors. No shell expansion or evaluation.
type Result<T> = std::result::Result<T, &'static str>;
const MAX_EXPRESSION: usize = 512;

fn finite(value: f64) -> Result<f64> {
    value
        .is_finite()
        .then_some(value)
        .ok_or("result is undefined or out of range")
}

fn render(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else if (1e-7..1e16).contains(&value.abs()) {
        value.to_string()
    } else {
        format!("{value:e}")
    }
}

struct Parser<'a> {
    rest: &'a str,
    answer: Option<f64>,
}

impl<'a> Parser<'a> {
    fn take(&mut self, token: &str) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix(token) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn expression(&mut self, minimum: u8, depth: u8) -> Result<f64> {
        if depth >= 40 {
            return Err("expression is nested too deeply");
        }
        let mut left = if self.take("+") {
            self.expression(5, depth + 1)?
        } else if self.take("-") {
            -self.expression(5, depth + 1)?
        } else if self.take("(") {
            let value = self.expression(0, depth + 1)?;
            if !self.take(")") {
                return Err("expected closing parenthesis");
            }
            value
        } else {
            self.atom(depth)?
        };

        loop {
            self.rest = self.rest.trim_start();
            if minimum <= 9 && self.take("!") {
                left = factorial(left)?;
                continue;
            }
            let (op, lbp, rbp) = match self.rest.as_bytes() {
                [b'*', b'*', ..] => ("**", 7, 6),
                [b'^', ..] => ("^", 7, 6),
                [b'*', ..] => ("*", 3, 4),
                [b'/', ..] => ("/", 3, 4),
                [b'%', ..] => ("%", 3, 4),
                [b'+', ..] => ("+", 1, 2),
                [b'-', ..] => ("-", 1, 2),
                _ => break,
            };
            if lbp < minimum {
                break;
            }
            self.rest = &self.rest[op.len()..];
            let right = self.expression(rbp, depth + 1)?;
            left = finite(match op {
                "+" => left + right,
                "-" => left - right,
                "*" => left * right,
                "/" => left / right,
                "%" => left % right,
                _ => left.powf(right),
            })?;
        }
        Ok(left)
    }

    fn atom(&mut self, depth: u8) -> Result<f64> {
        let name_len = self
            .rest
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
        if name_len == 0 {
            return self.number();
        }
        let (name, rest) = self.rest.split_at(name_len);
        self.rest = rest;
        if !self.take("(") {
            return match name {
                "pi" => Ok(std::f64::consts::PI),
                "tau" => Ok(std::f64::consts::TAU),
                "e" => Ok(std::f64::consts::E),
                "ans" => self.answer.ok_or("no previous answer"),
                _ => Err("unknown constant or missing function parentheses"),
            };
        }
        let arg = self.expression(0, depth + 1)?;
        let second = if self.take(",") {
            Some(self.expression(0, depth + 1)?)
        } else {
            None
        };
        if !self.take(")") {
            return Err("expected closing parenthesis after function arguments");
        }
        finite(match (name, second) {
            ("sqrt", None) => arg.sqrt(),
            ("cbrt", None) => arg.cbrt(),
            ("abs", None) => arg.abs(),
            ("sin", None) => arg.sin(),
            ("cos", None) => arg.cos(),
            ("tan", None) => arg.tan(),
            ("asin", None) => arg.asin(),
            ("acos", None) => arg.acos(),
            ("atan", None) => arg.atan(),
            ("atan", Some(x)) => arg.atan2(x),
            ("rad", None) => arg.to_radians(),
            ("deg", None) => arg.to_degrees(),
            ("ln", None) => arg.ln(),
            ("log", None) => arg.log10(),
            ("log", Some(base)) => arg.log(base),
            ("exp", None) => arg.exp(),
            ("floor", None) => arg.floor(),
            ("ceil", None) => arg.ceil(),
            ("round", None) => arg.round(),
            ("min", Some(x)) => arg.min(x),
            ("max", Some(x)) => arg.max(x),
            ("hypot", Some(x)) => arg.hypot(x),
            ("pow", Some(x)) => arg.powf(x),
            ("fact", None) => factorial(arg)?,
            _ => return Err("unknown function or wrong number of arguments"),
        })
    }

    fn number(&mut self) -> Result<f64> {
        let bytes = self.rest.as_bytes();
        let mut end = bytes
            .iter()
            .take_while(|c| c.is_ascii_digit() || **c == b'.')
            .count();
        if end == 0 {
            return Err("expected a number, constant, or function");
        }
        if matches!(bytes.get(end), Some(b'e' | b'E')) {
            end += 1;
            if matches!(bytes.get(end), Some(b'+' | b'-')) {
                end += 1;
            }
            end += bytes[end..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
        }
        let (number, rest) = self.rest.split_at(end);
        self.rest = rest;
        finite(number.parse().map_err(|_| "invalid number")?)
    }
}

fn factorial(value: f64) -> Result<f64> {
    if !(0.0..=170.0).contains(&value) || value.fract() != 0.0 {
        return Err("factorial requires an integer from 0 to 170");
    }
    Ok((1..=value as u32).fold(1.0, |product, n| product * f64::from(n)))
}

fn evaluate(expression: &str, answer: Option<f64>) -> Result<f64> {
    if expression.len() > MAX_EXPRESSION || !expression.is_ascii() {
        return Err("expected an ASCII expression of at most 512 bytes");
    }
    let mut parser = Parser {
        rest: expression.trim_start(),
        answer,
    };
    let value = parser.expression(0, 0)?;
    if !parser.rest.trim().is_empty() {
        return Err("unexpected text after expression");
    }
    finite(value)
}

fn math_text(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b" +-*/%^!.(),=".contains(&byte))
}

fn editor_edit(left: &str, right: &str, mut answer: Option<f64>) -> Option<(String, String)> {
    if left.len() + right.len() > MAX_EXPRESSION || !math_text(left) || !math_text(right) {
        return None;
    }
    let mut current = left.trim_start();
    let mut chained = false;
    while let Some((expression, rest)) = current.split_once('=') {
        if rest.is_empty() && right.trim().is_empty() {
            let rendered = render(evaluate(expression, answer).ok()?);
            return Some((rendered.clone(), rendered));
        }
        // Continue only past a verified, previously inserted equation.
        let (displayed, next) = rest.split_once(' ')?;
        let value = finite(displayed.parse().ok()?).ok()?;
        let evaluated = evaluate(expression, answer).ok()?;
        let used_answer = expression
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|name| name == "ans");
        if displayed != render(evaluated) && !(used_answer && answer == Some(value)) {
            return None;
        }
        answer = Some(value);
        current = next.trim_start();
        chained = true;
    }

    let answer = render(answer?);
    let entry = current.trim_end();
    let needs_operand = entry.ends_with(['(', '+', '-', '*', '/', '^', '%', ',']);
    let numeric_prefix = entry
        .trim_start_matches(['(', '+', '-'])
        .trim_start()
        .as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_digit() || *b == b'.');
    let empty_slot = right
        .trim_start()
        .bytes()
        .next()
        .is_none_or(|b| b"+-*/%^!,)=".contains(&b));
    if needs_operand && empty_slot && (chained || numeric_prefix) {
        // Parenthesize a negative operand so a later power binds to the whole answer.
        let suffix = if answer.starts_with('-') {
            format!("({answer})")
        } else {
            answer.clone()
        };
        return Some((answer, suffix));
    }
    None
}

pub(crate) fn edit(args: &[String]) -> i32 {
    let previous = args
        .first()
        .and_then(|value| value.parse().ok())
        .and_then(|n| finite(n).ok());
    let Some((left, right)) = crate::util::editor_input(&mut std::io::stdin().lock()) else {
        return 1;
    };
    let Some((answer, insertion)) = editor_edit(&left, &right, previous) else {
        return 1;
    };
    println!("{answer}\t{insertion}");
    0
}

pub(crate) fn run(args: &[String]) -> i32 {
    if args.is_empty() || args == ["--help"] {
        println!("usage: complete-me calc [--last NUMBER] 'EXPRESSION'");
        return if args.is_empty() { 2 } else { 0 };
    }
    let (answer, expression) = match args {
        [flag, previous, rest @ ..] if flag == "--last" => {
            let Some(value) = previous.parse().ok().and_then(|n| finite(n).ok()) else {
                eprintln!("complete-me calc: --last needs a finite number");
                return 2;
            };
            (Some(value), rest)
        }
        [flag, rest @ ..] if flag == "--" => (None, rest),
        _ => (None, args),
    };
    match evaluate(&expression.join(" "), answer) {
        Ok(value) => {
            println!("{}", render(value));
            0
        }
        Err(error) => {
            eprintln!("complete-me calc: {error}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_scientific_functions() {
        for (expression, value) in [
            ("2*34", 68.0),
            ("2^3**2", 512.0),
            ("-2^2", -4.0),
            ("2^-3", 0.125),
            ("3! + 1e-3*4000", 10.0),
            ("sqrt(144) + log(1000, 10)", 15.0),
            ("hypot(3,4) + round(sin(rad(90)))", 6.0),
        ] {
            assert_eq!(evaluate(expression, None), Ok(value), "{expression}");
        }
        assert_eq!(evaluate("ans % 5", Some(68.0)), Ok(3.0));
    }

    #[test]
    fn rejects_errors_without_any_shell_semantics() {
        for expression in [
            "",
            "2 3",
            "1/0",
            "sqrt(-1)",
            "171!",
            "1e999",
            "4 + foo(3)",
            "1 + (2",
            "5 + name",
        ] {
            assert!(evaluate(expression, None).is_err(), "{expression}");
        }
        assert!(evaluate(&"(".repeat(51), None).is_err());
        assert!(evaluate(&"2".repeat(513), None).is_err());
    }

    #[test]
    fn inserts_results_and_chains_answers_only_in_math_slots() {
        assert_eq!(
            editor_edit("2*34=", "", None),
            Some(("68".into(), "68".into()))
        );
        assert_eq!(
            editor_edit("2*34=68 (", "/3)^2=", None),
            Some(("68".into(), "68".into()))
        );
        assert_eq!(
            editor_edit("2*34=68 (68/3)^", "=", None),
            Some(("68".into(), "68".into()))
        );
        assert_eq!(
            editor_edit("2*34=68 (68/3)^2=", "", None).unwrap().0,
            render((68.0_f64 / 3.0).powf(2.0))
        );
        assert_eq!(editor_edit("ans+3=", "", Some(4.0)).unwrap().0, "7");
        assert_eq!(editor_edit("2^", "", Some(-3.0)).unwrap().1, "(-3)");
        for line in ["git config value=", "echo 2*34=", "2*34=69 (", "(", "1/0="] {
            assert!(editor_edit(line, "", Some(68.0)).is_none(), "{line}");
        }
    }
}
