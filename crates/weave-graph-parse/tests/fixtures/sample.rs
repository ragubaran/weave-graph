use std::collections::HashMap;

pub struct Greeter {
    name: String,
}

impl Greeter {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn greet(&self) -> String {
        format_name(&self.name)
    }
}

trait Farewell {
    fn bye(&self) -> String;
}

impl Farewell for Greeter {
    fn bye(&self) -> String {
        self.greet()
    }
}

fn format_name(name: &str) -> String {
    name.to_string()
}
