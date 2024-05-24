pub fn template(template: &str, vars: &[(&str, String)]) -> String {
    let mut template = template.to_string();
    for var in vars {
        template = template.replace(&format!("${{{}}}", var.0), &var.1);
    }
    template
}
