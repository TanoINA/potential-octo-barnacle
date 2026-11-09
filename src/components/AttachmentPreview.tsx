import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { Image as ImageIcon, X } from 'lucide-react';
import { useModalAccessibility } from '../hooks/useModalAccessibility';
import type { AttachedContext, ImageInput } from '../types/agent';

interface Props {
  attachment: AttachedContext;
  sessionId?: string | null;
  workspaceCwd: string;
  /** Selected model (or the known text-only council candidate). Unknown models validate at send. */
  model?: string;
  allowPreview?: boolean;
}

// Preview-only cache. Never bypass send preflight or retain object URLs globally.
const cache = new Map<string, ImageInput>();
const CACHE_BYTES = 20 * 1024 * 1024;
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
function remember(key: string, image: ImageInput) {
  cache.delete(key);
  cache.set(key, image);
  // Count UTF-16 base64 storage conservatively, not just decoded bytes.
  while (cache.size > 8 || [...cache.values()].reduce((sum, item) => sum + item.data.length * 2, 0) > CACHE_BYTES) {
    cache.delete(cache.keys().next().value!);
  }
}

export function AttachmentPreview(props: Props) {
  const identity = JSON.stringify([props.sessionId, props.workspaceCwd, props.attachment.id, props.attachment.path, props.attachment.name, props.attachment.isDir, props.allowPreview]);
  return <ScopedPreview key={identity} {...props} identity={identity} />;
}

function ScopedPreview({ attachment, model, identity, allowPreview = true }: Props & { identity: string }) {
  const supported = !attachment.isDir && /\.(png|jpe?g|gif|webp)$/i.test(attachment.path);
  const blocked = /^(deepseek-chat|deepseek-reasoner)$/i.test(model?.trim() ?? '');
  const [open, setOpen] = useState(false);
  const [requested, setRequested] = useState(false);
  const [src, setSrc] = useState<string>();
  const [error, setError] = useState<string>();
  const dialogRef = useModalAccessibility<HTMLDivElement>(open, () => setOpen(false));

  useEffect(() => {
    if (!requested || !supported || !allowPreview) return;
    let active = true;
    let objectUrl: string | undefined;
    void (async () => {
      try {
        const cached = cache.get(identity);
        const images = cached ? [cached] : await invoke<ImageInput[]>('prepare_image_attachments', { attachments: [attachment] });
        if (!active) return;
        const image = images?.[0];
        if (images?.length !== 1 || !image || image.sourcePath !== attachment.path
          || !/^image\/(png|jpeg|gif|webp)$/.test(image.mediaType)
          || !image.data || image.data.length > 4 * Math.ceil(MAX_IMAGE_BYTES / 3)
          || image.data.length % 4 !== 0 || /[^A-Za-z0-9+/=]/.test(image.data)) {
          throw new Error('Invalid image preview payload');
        }
        const decoded = atob(image.data);
        if (decoded.length > MAX_IMAGE_BYTES) throw new Error('Image preview exceeds 5 MiB');
        if (typeof URL.createObjectURL === 'function') {
          objectUrl = URL.createObjectURL(new Blob([Uint8Array.from(decoded, (char) => char.charCodeAt(0))], { type: image.mediaType }));
          setSrc(objectUrl);
        } else {
          setSrc(`data:${image.mediaType};base64,${image.data}`);
        }
        remember(identity, image);
      } catch (reason) {
        if (active) setError(reason instanceof Error ? reason.message : String(reason));
      }
    })();
    return () => {
      active = false;
      if (objectUrl && typeof URL.revokeObjectURL === 'function') URL.revokeObjectURL(objectUrl);
    };
  }, [requested, supported, identity, allowPreview]);

  if (!supported) return <span className="text-[10px] text-[#8b949e]" title="File context only; pixels are not sent for vision.">File context</span>;
  const status = blocked ? 'Vision blocked: text-only model' : 'Vision input';
  return <>
    {allowPreview && <button type="button" aria-label={`Preview ${attachment.name}`} title="Load image preview" data-attachment-preview
      onClick={() => { setOpen(true); setRequested(true); }}
      className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded border border-[#30363d] hover:border-[#58a6ff]">
      {src ? <img src={src} alt={attachment.name} className="h-full w-full object-cover" /> : <ImageIcon size={18} aria-hidden="true" />}
    </button>}
    <span className={`text-[10px] ${blocked ? 'text-[#f85149]' : 'text-[#8b949e]'}`}
      title={blocked ? `${model} does not support images. Choose a vision-capable model before sending.` : model ? 'PNG/JPEG/GIF/WebP pixels are prepared on send. Model capability is validated by the provider.' : 'Choose a vision-capable model before sending.'}>{status}</span>
    {open && createPortal(
      <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/80 p-6" onClick={(event) => { if (event.target === event.currentTarget) setOpen(false); }}>
        <div ref={dialogRef} role="dialog" aria-modal="true" aria-label={`Preview ${attachment.name}`} tabIndex={-1}
          className="flex max-h-[90vh] max-w-[90vw] flex-col gap-3 rounded-xl border border-[#30363d] bg-[#161b22] p-4 text-[#e6edf3] shadow-2xl">
          <div className="flex items-center justify-between gap-4"><span className="truncate">{attachment.name}</span>
            <button type="button" aria-label="Close image preview" onClick={() => setOpen(false)}><X size={20} /></button></div>
          {error ? <p role="alert">Preview unavailable: {error}</p> : src
            ? <img src={src} alt={attachment.name} className="min-h-0 max-h-[75vh] max-w-full object-contain" onError={() => setError('The image could not be displayed.')} />
            : <p role="status">Loading image preview...</p>}
          <p className="text-xs text-[#8b949e]">{blocked ? `${status}. Choose a vision-capable model before sending.` : status}</p>
        </div>
      </div>, document.body)}
  </>;
}
