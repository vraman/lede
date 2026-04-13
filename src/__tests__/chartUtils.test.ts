import { describe, it, expect } from "vitest";
import {
  truncLabel,
  tryParseDate,
  buildRows,
  aggregate,
  countByGroup,
  looksLikeUSStates,
  normalizeGeoName,
} from "../components/ChartView";
import { ParsedCSV } from "../types";

describe("truncLabel", () => {
  it("returns short strings unchanged", () => {
    expect(truncLabel("hello")).toBe("hello");
  });

  it("truncates long strings with ellipsis", () => {
    const long = "a".repeat(50);
    const result = truncLabel(long);
    expect(result.length).toBe(40);
    expect(result.endsWith("\u2026")).toBe(true);
  });

  it("respects custom max length", () => {
    expect(truncLabel("hello world", 5)).toBe("hell\u2026");
  });
});

describe("tryParseDate", () => {
  it("parses ISO dates", () => {
    const d = tryParseDate("2024-01-15");
    expect(d).not.toBeNull();
    expect(d!.getFullYear()).toBe(2024);
  });

  it("parses M/D/YYYY H:M:S format", () => {
    const d = tryParseDate("1/15/2024 14:30:00");
    expect(d).not.toBeNull();
    expect(d!.getFullYear()).toBe(2024);
    expect(d!.getMonth()).toBe(0); // January
  });

  it("returns null for garbage", () => {
    expect(tryParseDate("not a date")).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(tryParseDate("")).toBeNull();
  });
});

describe("buildRows", () => {
  const data: ParsedCSV = {
    columns: [
      { name: "name", col_type: "Categorical", unique_count: 2, total_count: 2, sample_values: [] },
      { name: "revenue", col_type: "NumericMeasure", unique_count: 2, total_count: 2, sample_values: [] },
    ],
    row_count: 2,
    sample_rows: [
      ["Alice", "100.5"],
      ["Bob", "200"],
    ],
    suggestions: [],
  };

  it("converts sample rows to objects", () => {
    const rows = buildRows(data);
    expect(rows).toHaveLength(2);
    expect(rows[0].name).toBe("Alice");
    expect(rows[1].name).toBe("Bob");
  });

  it("parses numeric columns as numbers", () => {
    const rows = buildRows(data);
    expect(rows[0].revenue).toBe(100.5);
    expect(rows[1].revenue).toBe(200);
  });

  it("keeps non-numeric columns as strings", () => {
    const rows = buildRows(data);
    expect(typeof rows[0].name).toBe("string");
  });
});

describe("aggregate", () => {
  const rows = [
    { region: "West", revenue: 100 },
    { region: "West", revenue: 200 },
    { region: "East", revenue: 50 },
  ];

  it("sums by group", () => {
    const result = aggregate(rows, "region", "revenue", "Sum");
    const west = result.find((r) => r.region === "West");
    expect(west!.revenue).toBe(300);
  });

  it("averages by group", () => {
    const result = aggregate(rows, "region", "revenue", "Avg");
    const west = result.find((r) => r.region === "West");
    expect(west!.revenue).toBe(150);
  });

  it("counts by group", () => {
    const result = aggregate(rows, "region", "revenue", "Count");
    const west = result.find((r) => r.region === "West");
    expect(west!.revenue).toBe(2);
  });
});

describe("countByGroup", () => {
  const rows = [
    { product: "Widget" },
    { product: "Widget" },
    { product: "Gadget" },
  ];

  it("counts occurrences per group", () => {
    const result = countByGroup(rows, "product");
    const widget = result.find((r) => r.product === "Widget");
    expect(widget!.count).toBe(2);
  });
});

describe("looksLikeUSStates", () => {
  it("matches state names", () => {
    expect(looksLikeUSStates(["California", "Texas", "New York"])).toBe(true);
  });

  it("matches state abbreviations", () => {
    expect(looksLikeUSStates(["CA", "TX", "NY"])).toBe(true);
  });

  it("rejects non-state values", () => {
    expect(looksLikeUSStates(["West", "Central", "East"])).toBe(false);
  });

  it("rejects when less than half match", () => {
    expect(looksLikeUSStates(["California", "Zone A", "Region B"])).toBe(false);
  });
});

describe("normalizeGeoName", () => {
  it("expands state abbreviations", () => {
    expect(normalizeGeoName("ca")).toBe("California");
    expect(normalizeGeoName("TX")).toBe("Texas");
  });

  it("title-cases regular names", () => {
    expect(normalizeGeoName("new york")).toBe("New York");
  });

  it("trims whitespace", () => {
    expect(normalizeGeoName("  California  ")).toBe("California");
  });
});
