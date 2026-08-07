use minijinja::Environment;

fn main() {
    let env = Environment::new();
    for expr in ["1 == 1", "'a' == 'a'", "'a' == 'b'", "true", "false"] {
        let tmpl = env
            .template_from_str(&format!("{{{{ {} }}}}", expr))
            .unwrap();
        let s = tmpl.render(minijinja::Value::UNDEFINED).unwrap();
        println!("{} renders as: {:?}", expr, s);
    }
}
