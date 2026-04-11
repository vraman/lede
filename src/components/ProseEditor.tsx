import { useEditor, EditorContent } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";

interface Props {
  content: string;
  onUpdate: (content: string) => void;
}

export default function ProseEditor({ content, onUpdate }: Props) {
  const editor = useEditor({
    extensions: [StarterKit],
    content,
    onUpdate: ({ editor }) => {
      onUpdate(editor.getHTML());
    },
  });

  return (
    <div className="prose-editor">
      <EditorContent editor={editor} />
    </div>
  );
}
