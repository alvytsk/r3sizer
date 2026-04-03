/**
 * Decode an image File into raw RGBA pixel data via a temporary canvas.
 *
 * Shared between ImageUpload (drag-and-drop / file picker) and the
 * toolbar "Open another image" button in App.tsx.
 */
export function loadImageAsRgba(
  file: File,
): Promise<{ data: Uint8Array; width: number; height: number }> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    const url = URL.createObjectURL(file);

    img.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = img.naturalWidth;
        canvas.height = img.naturalHeight;
        const ctx = canvas.getContext("2d")!;
        ctx.drawImage(img, 0, 0);
        const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
        resolve({
          data: new Uint8Array(imageData.data.buffer),
          width: canvas.width,
          height: canvas.height,
        });
      } catch (err) {
        reject(err);
      } finally {
        URL.revokeObjectURL(url);
      }
    };

    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error(`Failed to decode image: ${file.name}`));
    };

    img.src = url;
  });
}
