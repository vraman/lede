import { useState, useCallback, DragEvent } from "react";

interface Props {
  onFileDrop: (path: string) => void;
}

export default function DropZone({ onFileDrop }: Props) {
  const [isDragging, setIsDragging] = useState(false);

  const handleDragOver = useCallback((e: DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback(() => {
    setIsDragging(false);
  }, []);

  const handleDrop = useCallback(
    (e: DragEvent) => {
      e.preventDefault();
      setIsDragging(false);

      const files = e.dataTransfer.files;
      if (files.length > 0) {
        const file = files[0];
        if (file.name.endsWith(".csv")) {
          // In Tauri, we get the file path from the drop event
          // But web File API doesn't expose paths. We'll use Tauri's drag-drop event instead.
          // For now, use the file picker as fallback
          onFileDrop(file.name);
        }
      }
    },
    [onFileDrop]
  );

  return (
    <div
      className={`drop-zone ${isDragging ? "dragging" : ""}`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <div className="drop-zone-content">
        <h2>Lede</h2>
        <p className="tagline">Don't bury the lede in your data.</p>
        <p className="drop-hint">Drop a CSV file here, or click to browse</p>
      </div>
    </div>
  );
}
