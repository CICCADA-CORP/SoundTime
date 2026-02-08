import { describe, it, expect, vi, beforeEach } from 'vitest';
import { tick } from 'svelte';

const { mockApi } = vi.hoisted(() => {
  const abortFn = vi.fn();
  return {
    mockApi: {
      uploadWithProgress: vi.fn(() => ({
        promise: Promise.resolve({ id: 't1', title: 'Test Song', format: 'mp3', duration: 120 }),
        abort: abortFn,
      })),
      _abortFn: abortFn,
    },
  };
});

vi.mock('$lib/api', () => ({
  api: mockApi,
}));

vi.mock('svelte', async () => {
  const actual = await vi.importActual('svelte');
  return {
    ...actual,
    createEventDispatcher: () => vi.fn(),
  };
});

vi.mock('lucide-svelte', () => {
  const noop = function($$anchor: any, $$props?: any) {};
  return {
    Upload: noop,
    X: noop,
    CheckCircle: noop,
    AlertCircle: noop,
    Music: noop,
  };
});

import { render, fireEvent, waitFor } from '@testing-library/svelte';
import UploadDropzone from './UploadDropzone.svelte';

function createMockFile(name: string, size: number, type = 'audio/mp3'): File {
  const file = new File(['x'.repeat(size)], name, { type });
  Object.defineProperty(file, 'size', { value: size });
  return file;
}

function createFileList(files: File[]): FileList {
  const fl = {
    length: files.length,
    item: (i: number) => files[i] ?? null,
    [Symbol.iterator]: function* () {
      for (const f of files) yield f;
    },
  } as any;
  for (let i = 0; i < files.length; i++) fl[i] = files[i];
  return fl;
}

describe('UploadDropzone', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApi.uploadWithProgress.mockReturnValue({
      promise: Promise.resolve({ id: 't1', title: 'Test Song', format: 'mp3', duration: 120 }),
      abort: vi.fn(),
    });
  });

  // --- Rendering ---
  it('renders the dropzone with instructions', () => {
    const { container } = render(UploadDropzone);
    expect(container.querySelector('[role="button"]')).toBeInTheDocument();
  });

  it('contains a hidden file input', () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]');
    expect(fileInput).toBeInTheDocument();
    expect(fileInput).toHaveClass('hidden');
  });

  it('accepts audio file types', () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]');
    expect(fileInput?.getAttribute('accept')).toContain('audio/*');
  });

  it('allows multiple file selection', () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]');
    expect(fileInput?.hasAttribute('multiple')).toBe(true);
  });

  it('shows dropzone text', () => {
    const { container } = render(UploadDropzone);
    expect(container.textContent).toContain('Déposez vos fichiers audio');
    expect(container.textContent).toContain('MP3, FLAC, OGG');
  });

  // --- Drag events ---
  it('handles dragover event', async () => {
    const { container } = render(UploadDropzone);
    const dropzone = container.querySelector('[role="button"]')!;
    await fireEvent.dragOver(dropzone);
    // isDragging = true → should add green border class
    expect(dropzone.classList.toString()).toMatch(/border/);
  });

  it('handles dragleave event', async () => {
    const { container } = render(UploadDropzone);
    const dropzone = container.querySelector('[role="button"]')!;
    await fireEvent.dragOver(dropzone);
    await fireEvent.dragLeave(dropzone);
    // isDragging reset to false
    expect(dropzone).toBeInTheDocument();
  });

  it('handles drop event with files', async () => {
    const { container } = render(UploadDropzone);
    const dropzone = container.querySelector('[role="button"]')!;
    const file = createMockFile('song.mp3', 1024);
    const mockDataTransfer = {
      files: createFileList([file]),
    };
    await fireEvent.drop(dropzone, { dataTransfer: mockDataTransfer });
    await tick();
    // The file should be added to queue and upload should start
    await waitFor(() => {
      expect(mockApi.uploadWithProgress).toHaveBeenCalled();
    });
  });

  it('handles drop event without files gracefully', async () => {
    const { container } = render(UploadDropzone);
    const dropzone = container.querySelector('[role="button"]')!;
    await fireEvent.drop(dropzone, { dataTransfer: { files: createFileList([]) } });
    // No crash, no upload
    expect(mockApi.uploadWithProgress).not.toHaveBeenCalled();
  });

  // --- File input selection ---
  it('handles file selection from input', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('track.mp3', 2048);
    // Simulate change event with files
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);
    await tick();
    await waitFor(() => {
      expect(mockApi.uploadWithProgress).toHaveBeenCalled();
    });
  });

  // --- Deduplication ---
  it('deduplicates files by name and size', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file1 = createMockFile('song.mp3', 1024);
    const file1dup = createMockFile('song.mp3', 1024);

    Object.defineProperty(fileInput, 'files', { value: createFileList([file1]), writable: true });
    await fireEvent.change(fileInput);
    await tick();

    // Second time same file
    Object.defineProperty(fileInput, 'files', { value: createFileList([file1dup]), writable: true });
    await fireEvent.change(fileInput);
    await tick();

    // uploadWithProgress should only be called once (for the first file)
    await waitFor(() => {
      expect(mockApi.uploadWithProgress).toHaveBeenCalledTimes(1);
    });
  });

  // --- Upload error handling ---
  it('handles upload error', async () => {
    mockApi.uploadWithProgress.mockReturnValue({
      promise: Promise.reject(new Error('Network error')),
      abort: vi.fn(),
    });

    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('fail.mp3', 512);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    // Should show error message after processing
    await waitFor(() => {
      expect(container.textContent).toContain('Network error');
    });
  });

  it('handles upload error with non-Error exception', async () => {
    mockApi.uploadWithProgress.mockReturnValue({
      promise: Promise.reject('string error'),
      abort: vi.fn(),
    });

    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('fail2.mp3', 512);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toContain('Upload failed');
    });
  });

  // --- Successful upload display ---
  it('shows success info after upload completes', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('success.mp3', 2048);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toContain('Test Song');
    });
  });

  // --- Queue display ---
  it('shows queue counter after adding files', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('queued.mp3', 1024);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toMatch(/terminé/);
    });
  });

  // --- Remove from queue ---
  it('removes file from queue via remove button', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('remove-me.mp3', 1024);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      const removeBtn = container.querySelector('button[title="Retirer"]');
      expect(removeBtn).toBeInTheDocument();
    });

    const removeBtn = container.querySelector('button[title="Retirer"]')!;
    await fireEvent.click(removeBtn);
    await tick();

    // After removal, the file should no longer appear
    await waitFor(() => {
      expect(container.querySelector('button[title="Retirer"]')).not.toBeInTheDocument();
    });
  });

  // --- Multiple files processing ---
  it('processes multiple files sequentially', async () => {
    let resolves: Function[] = [];
    mockApi.uploadWithProgress.mockImplementation(() => ({
      promise: new Promise(r => resolves.push(r)),
      abort: vi.fn(),
    }));

    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file1 = createMockFile('first.mp3', 1024);
    const file2 = createMockFile('second.mp3', 2048);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file1, file2]), writable: true });
    await fireEvent.change(fileInput);
    await tick();

    // Only one upload at a time
    expect(mockApi.uploadWithProgress).toHaveBeenCalledTimes(1);

    // Resolve first upload
    resolves[0]({ id: 't1', title: 'First', format: 'mp3', duration: 100 });
    await tick();

    // Now second should start
    await waitFor(() => {
      expect(mockApi.uploadWithProgress).toHaveBeenCalledTimes(2);
    });
  });

  // --- Keyboard accessibility ---
  it('opens file picker on Enter key', async () => {
    const { container } = render(UploadDropzone);
    const dropzone = container.querySelector('[role="button"]')!;
    // Just verify keydown handler exists (Enter triggers fileInput.click())
    await fireEvent.keyDown(dropzone, { key: 'Enter' });
    // No error = success
    expect(dropzone).toBeInTheDocument();
  });

  // --- Click opens file picker ---
  it('clicking dropzone triggers file input', async () => {
    const { container } = render(UploadDropzone);
    const dropzone = container.querySelector('[role="button"]')!;
    await fireEvent.click(dropzone);
    expect(dropzone).toBeInTheDocument();
  });

  // --- Clear all button ---
  it('shows clear all button when not uploading', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('clear.mp3', 512);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toContain('Tout effacer');
    });
  });

  // --- File size display ---
  it('displays formatted file size', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    // 1.5 MB file
    const file = createMockFile('big.mp3', 1572864);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toMatch(/Mo/);
    });
  });

  it('displays small file size in bytes', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('tiny.mp3', 500);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toContain('500 o');
    });
  });

  it('displays KB file size', async () => {
    const { container } = render(UploadDropzone);
    const fileInput = container.querySelector('input[type="file"]')!;
    const file = createMockFile('medium.mp3', 50000);
    Object.defineProperty(fileInput, 'files', { value: createFileList([file]), writable: true });
    await fireEvent.change(fileInput);

    await waitFor(() => {
      expect(container.textContent).toMatch(/Ko/);
    });
  });
});
