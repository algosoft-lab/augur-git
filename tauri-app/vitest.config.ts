import { defineConfig } from 'vitest/config';

// The unit tests cover the pure helpers plus the theme writer, which touches
// the document, so the default environment is jsdom.
export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'packaging/**/*.test.ts']
  }
});
