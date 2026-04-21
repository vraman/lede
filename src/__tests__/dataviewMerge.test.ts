import { describe, it, expect } from "vitest";
import { mergeViewTypeSwap } from "../components/DataviewNodeView";
import { ViewSuggestion } from "../types";

const base: ViewSuggestion = {
  view_type: "BarChart",
  title: "Revenue by region",
  metric: "revenue",
  group_by: "region",
  aggregate: "Sum",
  sort: null,
  limit: null,
  x_col: null,
  y_col: null,
  shown_by_default: true,
  suggested_by: "ai",
  insight: "West region drives 62% of revenue.",
  insight_edited: false,
  denominator: null,
  filter_column: null,
  filter_value: null,
};

const altLine: ViewSuggestion = {
  ...base,
  view_type: "LineChart",
  suggested_by: "default",
  insight: null,
  insight_edited: false,
};

describe("mergeViewTypeSwap", () => {
  it("preserves AI insight when swapping view type", () => {
    const merged = mergeViewTypeSwap(base, altLine);
    expect(merged.view_type).toBe("LineChart");
    expect(merged.insight).toBe("West region drives 62% of revenue.");
    expect(merged.insight_edited).toBe(false);
    expect(merged.suggested_by).toBe("ai");
  });

  it("keeps user-edited insight flag across swap", () => {
    const edited = { ...base, insight: "My edit.", insight_edited: true };
    const merged = mergeViewTypeSwap(edited, altLine);
    expect(merged.insight).toBe("My edit.");
    expect(merged.insight_edited).toBe(true);
  });

  it("adopts view-specific fields from the target", () => {
    const merged = mergeViewTypeSwap(base, { ...altLine, aggregate: "Avg" });
    expect(merged.aggregate).toBe("Avg");
    expect(merged.view_type).toBe("LineChart");
  });
});
