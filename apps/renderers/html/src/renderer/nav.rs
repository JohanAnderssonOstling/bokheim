use super::*;

impl HTMLRenderer {
    pub fn resolve_href(&self, href: &str) -> Option<(usize, Option<String>)> {
        self.renderer.resolve_href(href)
    }

    pub fn navigate_to_href(&mut self, href: &str) -> bool {
        self.renderer.navigate_to_href(href)
    }

    pub fn restore_position_from_cfi(&mut self, cfi: &str) -> bool {
        self.renderer.restore_position_from_cfi(cfi)
    }
}
