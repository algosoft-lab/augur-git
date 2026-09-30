import { describe, expect, it } from 'vitest';

import type { FileChange, ImagePreviewTarget } from '../../bridge/types';
import { canPreviewImage, isSvgPreview } from './imagePreview';

function change(patch: Partial<FileChange> = {}): FileChange {
  return {
    path: 'assets/logo.svg',
    old_path: null,
    new_path: 'assets/logo.svg',
    status: 'modified',
    old_blob: null,
    new_blob: null,
    added: 1,
    deleted: 1,
    ...patch
  };
}

describe('image preview type detection', () => {
  it.each(['png', 'jpg', 'jpeg', 'ico', 'svg'])('supports .%s paths', (extension) => {
    const target: ImagePreviewTarget = {
      kind: 'change',
      file: change({ new_path: `assets/image.${extension}`, old_path: `assets/image.${extension}` })
    };
    expect(canPreviewImage(target)).toBe(true);
  });

  it('matches extensions without regard to case and excludes unsupported files', () => {
    expect(
      canPreviewImage({
        kind: 'change',
        file: change({ new_path: 'assets/IMAGE.PNG', old_path: null })
      })
    ).toBe(true);
    expect(
      canPreviewImage({
        kind: 'change',
        file: change({ new_path: 'assets/document.pdf', old_path: null })
      })
    ).toBe(false);
  });

  it('recognizes SVG renames from either side', () => {
    const target: ImagePreviewTarget = {
      kind: 'change',
      file: change({
        old_path: 'assets/mark.svg',
        new_path: 'assets/mark.xml',
        status: 'renamed'
      })
    };
    expect(canPreviewImage(target)).toBe(true);
    expect(isSvgPreview(target)).toBe(true);
  });
});
