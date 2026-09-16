import importlib.util
import unittest
from pathlib import Path

MODULE = Path(__file__).resolve().parent / 'quality' / 'check_action_handlers.py'
spec = importlib.util.spec_from_file_location('check_action_handlers', MODULE)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


HANDLED = '''
fn build(&self, cx: &mut Cx) -> Screen {
    Screen::new()
        .top_bar_action("refresh", "Refresh")
        .primary_button("save", "Save")
        .rows([("entry-0", "First", "", Glyph::Book)])
}
fn on_action(&mut self, action: &str, cx: &mut Cx) {
    if action == action_id("refresh") { return; }
    if action == action_id("save") { return; }
}
'''

UNCLAIMED = '''
fn build(&self, cx: &mut Cx) -> Screen {
    Screen::new()
        .primary_button("save", "Save")
        .rows([("mystery", "A row", "", Glyph::Book)])
}
fn on_action(&mut self, action: &str, cx: &mut Cx) {
    if action == action_id("save") { return; }
}
'''

TEST_MODULE = '''
fn on_action(&mut self, action: &str, cx: &mut Cx) {}
#[cfg(test)]
mod tests {
    fn fake() {
        let _ = action_id("test-only-action");
    }
}
'''

PREFIX = '''
fn build(&self, cx: &mut Cx) -> Screen {
    Screen::new().rows([(format!("card-{}", i), "Card", "", Glyph::Book)])
}
fn on_action(&mut self, action: &str, cx: &mut Cx) {
    if let Some(rest) = action.strip_prefix("card-") { return; }
}
'''

HELPER_PREFIX = '''
fn build(&self, cx: &mut Cx) -> Screen {
    Screen::new().grid([(format!("cell-{}", i), "Cell")])
}
fn on_action(&mut self, action: &str, cx: &mut Cx) {
    if let Some(n) = indexed(action, "cell-") { return; }
}
'''


class CheckActionHandlers(unittest.TestCase):
    def names(self, text):
        lits, prefixes, delegates = audit.handled(audit.strip_test_modules(text))
        return lits, prefixes, delegates

    def test_literal_actions_are_claimed(self):
        text = audit.strip_test_modules(HANDLED)
        lits, prefixes, _ = self.names(HANDLED)
        found = [n for kind, n in audit.emitted(text) if n not in lits]
        self.assertIn('refresh', lits)
        self.assertIn('save', lits)
        self.assertIn('entry-0', found, 'unhandled row id must be reported')

    def test_unclaimed_literal_surfaces(self):
        text = audit.strip_test_modules(UNCLAIMED)
        lits, _, _ = self.names(UNCLAIMED)
        unseen = [n for kind, n in audit.emitted(text) if n not in lits]
        self.assertIn('mystery', unseen)

    def test_test_modules_are_stripped(self):
        text = audit.strip_test_modules(TEST_MODULE)
        self.assertNotIn('test-only-action', text)

    def test_format_prefix_matches(self):
        lits, prefixes, _ = self.names(PREFIX)
        self.assertIn('card-', prefixes)

    def test_helper_prefix_matches(self):
        _, prefixes, _ = self.names(HELPER_PREFIX)
        self.assertIn('cell-', prefixes)


if __name__ == '__main__':
    unittest.main()
