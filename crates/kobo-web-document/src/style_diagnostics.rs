//! Fixed-size, content-free evidence for a refused visual parse.
//! Counts are occurrences, not unique defects, tests or compatibility scores.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum RefusalReason {
    InputBytes,
    DomNodes,
    DroppedAttributes,
    DroppedText,
    Encoding,
    Quirks,
    MissingStylesheet,
    StylesheetCount,
    StylesheetMedia,
    StylesheetEncoding,
    CssScanBudget,
    CssMalformed,
    CssAtRule,
    CssCondition,
    CssSelector,
    CssSelectorGroup,
    CssDeclaration,
    ExpandedRules,
    MatchWork,
    MatchedDeclarations,
    InlineDeclarations,
    RetainedNodes,
    ComputedValue,
    /// NEW supply-budget gate: summed decoded supplied-sheet bytes over
    /// `SupplyBudget::max_aggregate_bytes`. Never recorded under CANONICAL.
    SupplyAggregate,
}
impl RefusalReason {
    pub const ALL: [Self; 24] = [
        Self::InputBytes,
        Self::DomNodes,
        Self::DroppedAttributes,
        Self::DroppedText,
        Self::Encoding,
        Self::Quirks,
        Self::MissingStylesheet,
        Self::StylesheetCount,
        Self::StylesheetMedia,
        Self::StylesheetEncoding,
        Self::CssScanBudget,
        Self::CssMalformed,
        Self::CssAtRule,
        Self::CssCondition,
        Self::CssSelector,
        Self::CssSelectorGroup,
        Self::CssDeclaration,
        Self::ExpandedRules,
        Self::MatchWork,
        Self::MatchedDeclarations,
        Self::InlineDeclarations,
        Self::RetainedNodes,
        Self::ComputedValue,
        Self::SupplyAggregate,
    ];
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StyleDiagnostics {
    first: Option<RefusalReason>,
    counts: [usize; 24],
    /// Original byte length, including any unread tail.
    pub input_bytes: usize,
    /// DOM nodes created from the consumed prefix. Not necessarily complete.
    pub dom_nodes: usize,
    /// Style nodes retained before refusal. Not a faithful styled-DOM claim.
    pub retained_nodes: usize,
    pub dropped_attributes: usize,
    pub dropped_text: bool,
    /// Bytes of oversize `data:text/css` link hrefs retained under an opt-in or measurement cap.
    pub retained_inline_sheet_href_bytes: usize,
    /// Selector/property pairs retained, not unique selectors or CSS rules.
    pub expanded_declarations: usize,
    pub match_attempts: usize,
}
impl StyleDiagnostics {
    #[must_use]
    pub const fn first_reason(&self) -> Option<RefusalReason> {
        self.first
    }
    #[must_use]
    pub const fn count(&self, reason: RefusalReason) -> usize {
        self.counts[reason as usize]
    }
    pub(crate) fn record(&mut self, reason: RefusalReason) {
        self.first.get_or_insert(reason);
        self.counts[reason as usize] = self.counts[reason as usize].saturating_add(1);
    }
    // Input/DOM evidence precedes stylesheet/style evidence in the public
    // pipeline even though the parser finishes the retained arena first.
    pub(crate) fn prepend(&mut self, earlier: &Self) {
        self.first = earlier.first.or(self.first);
        for (count, extra) in self.counts.iter_mut().zip(earlier.counts) {
            *count = count.saturating_add(extra);
        }
        self.input_bytes = earlier.input_bytes;
        self.dom_nodes = earlier.dom_nodes;
        self.dropped_attributes = earlier.dropped_attributes;
        self.dropped_text = earlier.dropped_text;
        self.retained_inline_sheet_href_bytes = earlier.retained_inline_sheet_href_bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn occurrence_counters_saturate_and_first_follows_pipeline() {
        let mut css = StyleDiagnostics::default();
        css.record(RefusalReason::MissingStylesheet);
        css.counts[RefusalReason::MissingStylesheet as usize] = usize::MAX;
        css.record(RefusalReason::MissingStylesheet);
        assert_eq!(css.count(RefusalReason::MissingStylesheet), usize::MAX);
        let mut dom = StyleDiagnostics::default();
        dom.record(RefusalReason::DroppedAttributes);
        dom.dropped_attributes = 16;
        css.prepend(&dom);
        assert_eq!(css.first_reason(), Some(RefusalReason::DroppedAttributes));
        assert_eq!(css.count(RefusalReason::DroppedAttributes), 1);
        assert_eq!(css.dropped_attributes, 16);
        assert_eq!(css.count(RefusalReason::MissingStylesheet), usize::MAX);
    }
}
