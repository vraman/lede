import { useEffect } from "react";
import { useEditor, EditorContent, JSONContent } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { DataviewNode } from "../extensions/DataviewNode";
import { Editor } from "@tiptap/react";

interface Props {
  initialContent: JSONContent;
  onUpdate: (json: JSONContent) => void;
  editorRef?: (editor: Editor | null) => void;
}

export default function ProseEditor({
  initialContent,
  onUpdate,
  editorRef,
}: Props) {
  const editor = useEditor({
    extensions: [StarterKit, DataviewNode],
    content: initialContent,
    onUpdate: ({ editor }) => {
      onUpdate(editor.getJSON());
    },
  });

  // Expose editor instance to parent
  useEffect(() => {
    editorRef?.(editor);
    return () => editorRef?.(null);
  }, [editor]);

  // When initialContent identity changes (new file loaded), replace document
  useEffect(() => {
    if (!editor) return;
    const current = JSON.stringify(editor.getJSON());
    const next = JSON.stringify(initialContent);
    if (current !== next) {
      editor.commands.setContent(initialContent);
    }
  }, [initialContent, editor]);

  return (
    <div className="prose-editor">
      <EditorContent editor={editor} />
    </div>
  );
}
