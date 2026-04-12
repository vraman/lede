import { Node } from "@tiptap/react";
import { ReactNodeViewRenderer } from "@tiptap/react";
import DataviewNodeView from "../components/DataviewNodeView";

export const DataviewNode = Node.create({
  name: "dataview",
  group: "block",
  atom: true,
  draggable: true,

  addAttributes() {
    return {
      viewConfig: {
        default: null,
        parseHTML: (element) => {
          const raw = element.getAttribute("data-view-config");
          return raw ? JSON.parse(raw) : null;
        },
        renderHTML: (attributes) => {
          return {
            "data-view-config": JSON.stringify(attributes.viewConfig),
          };
        },
      },
    };
  },

  parseHTML() {
    return [{ tag: 'div[data-type="dataview"]' }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["div", { ...HTMLAttributes, "data-type": "dataview" }, 0];
  },

  addNodeView() {
    return ReactNodeViewRenderer(DataviewNodeView);
  },
});
