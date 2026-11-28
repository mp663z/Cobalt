#!/usr/bin/env python3
"""Negative contracts for the painted-session claim, no simulator required."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('session', Path(__file__).with_name('check-browser-session-sim.py'))
session = importlib.util.module_from_spec(spec)
spec.loader.exec_module(session)

class PaintedClaimTests(unittest.TestCase):
    def setUp(self):
        self.layout = {'paints': 9, 'nodes': [
            {'kind': 'Picture(PictureHandle(2147418113))'},
            {'kind': 'AddressValue(false, false)', 'lines': ['https://session.example.test/page/0']},
        ]}
        self.metadata = {'paints': '9'}

    def test_string_capture_generation_matches_numeric_layout_generation(self):
        session.verify_painted(self.layout, self.metadata, 0)

    def test_native_or_wrong_picture_cannot_satisfy_painted_claim(self):
        for kind in ['RichText', 'Picture(PictureHandle(1))', 'PictureLink(ActionId(1))']:
            layout = copy.deepcopy(self.layout)
            layout['nodes'][0]['kind'] = kind
            with self.assertRaises(AssertionError):
                session.verify_painted(layout, self.metadata, 0)

    def test_generation_and_url_must_match_the_capture(self):
        with self.assertRaises(AssertionError):
            session.verify_painted(self.layout, {'paints': '10'}, 0)
        with self.assertRaises(AssertionError):
            session.verify_painted(self.layout, self.metadata, 1)

    def test_native_prose_cannot_hide_beneath_a_painted_picture(self):
        for kind in ['Text', 'Heading', 'RichText']:
            layout = copy.deepcopy(self.layout)
            layout['nodes'].append({'kind': kind})
            with self.assertRaises(AssertionError):
                session.verify_painted(layout, self.metadata, 0)

if __name__ == '__main__':
    unittest.main()
