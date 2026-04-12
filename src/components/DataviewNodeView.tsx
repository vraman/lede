import { useState } from "react";
import { NodeViewWrapper, NodeViewProps } from "@tiptap/react";
import { useDataContext } from "../DataContext";
import { ViewSuggestion } from "../types";
import SummaryCard from "./SummaryCard";
import ChartView from "./ChartView";

export default function DataviewNodeView({
  node,
  updateAttributes,
  deleteNode,
  editor,
  getPos,
}: NodeViewProps) {
  const { data, suggestions } = useDataContext();
  const [showInsertMenu, setShowInsertMenu] = useState(false);
  const viewConfig = node.attrs.viewConfig as ViewSuggestion | null;

  if (!data || !viewConfig) return null;

  const isSummary = viewConfig.view_type === "SummaryCard";

  // Compute alternative view types for this metric/group_by
  const alternatives = suggestions.filter(
    (alt) =>
      alt.view_type !== viewConfig.view_type &&
      alt.metric === viewConfig.metric &&
      alt.group_by === viewConfig.group_by &&
      alt.view_type !== "SummaryCard"
  );

  // Views available to insert (non-default suggestions not already in doc)
  const insertable = suggestions.filter((s) => !s.shown_by_default);

  const handleSwap = (alt: ViewSuggestion) => {
    updateAttributes({ viewConfig: alt });
  };

  const handleChangeAggregate = (agg: ViewSuggestion["aggregate"]) => {
    updateAttributes({ viewConfig: { ...viewConfig, aggregate: agg } });
  };

  const handleInsert = (suggestion: ViewSuggestion) => {
    setShowInsertMenu(false);
    const pos = typeof getPos === "function" ? getPos() : null;
    if (pos == null || !editor) return;
    const endPos = pos + node.nodeSize;
    editor
      .chain()
      .focus()
      .insertContentAt(endPos, [
        { type: "paragraph" },
        {
          type: "dataview",
          attrs: { viewConfig: suggestion },
        },
        { type: "paragraph" },
      ])
      .run();
  };

  return (
    <NodeViewWrapper className="dataview-wrapper" data-drag-handle="">
      <div className="dataview-controls">
        <span className="dataview-drag-handle" data-drag-handle="">
          &#x2630;
        </span>
        <button
          className="dataview-remove"
          onClick={() => deleteNode()}
          title="Remove view"
        >
          &times;
        </button>
      </div>

      <div className="dataview-content">
        {isSummary ? (
          <SummaryCard data={data} suggestion={viewConfig} />
        ) : (
          <ChartView
            data={data}
            suggestion={viewConfig}
            alternatives={alternatives}
            onSwap={handleSwap}
            onChangeAggregate={handleChangeAggregate}
          />
        )}
      </div>

      <div className="dataview-insert-zone">
        <button
          className="dataview-insert-btn"
          onClick={() => setShowInsertMenu(!showInsertMenu)}
          title="Insert view below"
        >
          +
        </button>
        {showInsertMenu && insertable.length > 0 && (
          <div className="insert-menu">
            {insertable.map((s, i) => {
              const label =
                s.view_type === "SummaryCard"
                  ? `Summary: ${s.metric}`
                  : s.view_type === "ScatterPlot"
                    ? `${s.view_type}: ${s.x_col} vs ${s.y_col}`
                    : `${s.view_type}: ${s.metric || s.group_by || "view"}`;
              return (
                <button
                  key={i}
                  className="insert-menu-item"
                  onClick={() => handleInsert(s)}
                >
                  {label}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </NodeViewWrapper>
  );
}
