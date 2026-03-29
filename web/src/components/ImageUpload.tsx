import { useCallback, useRef, useState } from "react";
import { Upload } from "lucide-react";
import { useProcessorStore } from "@/stores/processor-store";

const ACCEPTED = ".png,.jpg,.jpeg,.bmp,.webp,.gif,.tiff";

export function ImageUpload() {
  const setInput = useProcessorStore((s) => s.setInput);
  const inputRef = useRef<HTMLInputElement>(null);
  const [isDragging, setIsDragging] = useState(false);

  const handleFile = useCallback(
    (file: File) => {
      const img = new Image();
      const url = URL.createObjectURL(file);

      img.onload = () => {
        const canvas = document.createElement("canvas");
        canvas.width = img.naturalWidth;
        canvas.height = img.naturalHeight;
        const ctx = canvas.getContext("2d")!;
        ctx.drawImage(img, 0, 0);
        const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
        setInput(
          file,
          new Uint8Array(imageData.data.buffer),
          canvas.width,
          canvas.height
        );
        URL.revokeObjectURL(url);
      };
      img.src = url;
    },
    [setInput]
  );

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);
      const file = e.dataTransfer.files[0];
      if (file) handleFile(file);
    },
    [handleFile]
  );

  const onChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (file) handleFile(file);
    },
    [handleFile]
  );

  return (
    <div
      className={`
        relative cursor-pointer rounded-lg border border-dashed transition-all duration-200
        ${isDragging
          ? "border-primary bg-primary/5 glow-amber"
          : "upload-idle hover:border-primary/60 hover:bg-surface/50"
        }
      `}
      onDragOver={(e) => { e.preventDefault(); setIsDragging(true); }}
      onDragLeave={() => setIsDragging(false)}
      onDrop={onDrop}
      onClick={() => inputRef.current?.click()}
    >
      <div className="flex flex-col items-center justify-center py-14 px-6 gap-3">
        <div className={`
          rounded-full p-3 transition-colors duration-200
          ${isDragging ? "bg-primary/10 text-primary" : "bg-surface text-muted-foreground"}
        `}>
          <Upload className="h-5 w-5" />
        </div>
        <div className="text-center space-y-1">
          <p className="text-sm font-medium text-foreground/80">
            Drop an image or click to upload
          </p>
          <p className="text-[11px] font-mono text-muted-foreground/60">
            PNG, JPEG, BMP, WebP, GIF, TIFF
          </p>
        </div>
      </div>
      <input
        ref={inputRef}
        type="file"
        accept={ACCEPTED}
        className="hidden"
        onChange={onChange}
      />
    </div>
  );
}
