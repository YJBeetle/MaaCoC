/** Keep the displayed frame while the next one decodes, and release Blob URLs
 * when they are replaced. A canceled decode must never overwrite a newer frame. */
export class PreviewImage {
  private source = "";
  private activeUrl: string | null = null;
  private cancelPending: (() => void) | null = null;

  constructor(private image: HTMLImageElement, private onLoad: () => void) {}

  update(source: string) {
    if (source === this.source) return;
    this.cancelPending?.();
    this.source = source;

    const ownedUrl = source.startsWith("data:") ? this.createUrl(source) : null;
    const url = ownedUrl ?? source;
    const pending = new Image();
    this.cancelPending = () => {
      pending.onload = null;
      pending.onerror = null;
      pending.removeAttribute("src");
      if (ownedUrl) URL.revokeObjectURL(ownedUrl);
      this.cancelPending = null;
    };
    pending.onload = () => {
      const previous = this.activeUrl;
      pending.onload = null;
      pending.onerror = null;
      this.cancelPending = null;
      this.image.src = url;
      this.activeUrl = ownedUrl;
      if (previous) URL.revokeObjectURL(previous);
      this.onLoad();
    };
    pending.onerror = () => {
      this.cancelPending?.();
      this.source = "";
    };
    pending.src = url;
  }

  clear() {
    this.cancelPending?.();
    if (this.activeUrl) URL.revokeObjectURL(this.activeUrl);
    this.activeUrl = null;
    this.source = "";
    this.image.removeAttribute("src");
  }

  private createUrl(source: string) {
    const comma = source.indexOf(",");
    const header = source.slice(5, comma);
    const data = source.slice(comma + 1);
    const binary = header.endsWith(";base64") ? atob(data) : decodeURIComponent(data);
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    return URL.createObjectURL(new Blob([bytes], { type: header.split(";")[0] }));
  }
}
