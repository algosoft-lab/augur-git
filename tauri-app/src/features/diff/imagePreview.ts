import type { ImagePreviewTarget } from '../../bridge/types';

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'ico', 'svg']);

function hasSupportedExtension(path: string): boolean {
  const parts = path.split('.');
  const extension = parts[parts.length - 1]?.toLowerCase();
  return extension !== undefined && IMAGE_EXTENSIONS.has(extension);
}

export function canPreviewImage(target: ImagePreviewTarget): boolean {
  if (target.kind === 'change') {
    const file = target.file;
    const oldPath = file.old_path ?? file.new_path;
    return (
      (file.status !== 'added' && hasSupportedExtension(oldPath)) ||
      (file.status !== 'deleted' && hasSupportedExtension(file.new_path))
    );
  }
  const file = target.file;
  const oldPath = file.old_path ?? file.path;
  return hasSupportedExtension(oldPath) || hasSupportedExtension(file.path);
}

export function isSvgPreview(target: ImagePreviewTarget): boolean {
  if (target.kind === 'change') {
    const file = target.file;
    return (
      (file.status !== 'added' &&
        (file.old_path ?? file.new_path).toLowerCase().endsWith('.svg')) ||
      (file.status !== 'deleted' && file.new_path.toLowerCase().endsWith('.svg'))
    );
  }
  const file = target.file;
  return (
    (file.old_path ?? file.path).toLowerCase().endsWith('.svg') ||
    file.path.toLowerCase().endsWith('.svg')
  );
}
