import type { CanvasMessage, HostMessage } from "./types";

declare global {
  interface Window {
    ipc?: {
      postMessage(message: string): void;
    };
  }
}

const HOST_EVENT_NAME = "one-message";

export function sendToHost(message: CanvasMessage): void {
  const payload = JSON.stringify(message);
  if (window.ipc?.postMessage) {
    window.ipc.postMessage(payload);
    return;
  }

  window.dispatchEvent(
    new CustomEvent("one-dev-ipc", {
      detail: message
    })
  );
}

export function subscribeHostMessages(
  callback: (message: HostMessage) => void
): () => void {
  const listener = (event: Event) => {
    const detail = (event as CustomEvent<unknown>).detail;
    if (!detail || typeof detail !== "object") {
      return;
    }

    const message = detail as HostMessage;
    if (message.type === "workflow:load") {
      callback(message);
    }
  };

  window.addEventListener(HOST_EVENT_NAME, listener);
  return () => window.removeEventListener(HOST_EVENT_NAME, listener);
}
