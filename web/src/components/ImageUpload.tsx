import { useCallback, useRef } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Upload } from "lucide-react";
import { useProcessorStore } from "@/stores/processor-store";

const ACCEPTED = ".png,.jpg,.jpeg,.bmp,.webp,.gif,.tiff";

export function ImageUpload() {
  const setInput = useProcessorStore((s) => s.setInput);
  const inputRef = useRef<HTMLInputElement>(null);

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
    <Card
      className="border-dashed border-2 cursor-pointer hover:border-primary/50 transition-colors"
      onDragOver={(e) => e.preventDefault()}
      onDrop={onDrop}
      onClick={() => inputRef.current?.click()}
    >
      <CardContent className="flex flex-col items-center justify-center py-8 gap-2 text-muted-foreground">
        <Upload className="h-8 w-8" />
        <p className="text-sm font-medium">Drop an image or click to upload</p>
        <p className="text-xs">PNG, JPEG, BMP, WebP, GIF, TIFF</p>
        <input
          ref={inputRef}
          type="file"
          accept={ACCEPTED}
          className="hidden"
          onChange={onChange}
        />
      </CardContent>
    </Card>
  );
}
