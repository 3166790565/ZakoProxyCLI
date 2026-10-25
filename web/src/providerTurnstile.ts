export type TurnstileOptions = {
  sitekey: string;
  callback: (token: string) => void;
  "error-callback": (code: string) => void;
  "expired-callback": () => void;
  "timeout-callback"?: () => void;
  "unsupported-callback"?: () => void;
  retry?: "auto" | "never";
  "refresh-expired"?: "auto" | "manual" | "never";
  "refresh-timeout"?: "auto" | "manual" | "never";
  "response-field"?: boolean;
  size?: "normal" | "compact" | "flexible";
};
export type TurnstileApi = {
  render: (element: HTMLElement, options: TurnstileOptions) => string;
  reset: (widgetId?: string) => void;
  remove: (widgetId: string) => void;
};
declare global { interface Window { turnstile?: TurnstileApi } }

let scriptPromise: Promise<TurnstileApi> | null = null;
export function loadProviderTurnstile(): Promise<TurnstileApi> {
  if (window.turnstile) return Promise.resolve(window.turnstile);
  if (scriptPromise) return scriptPromise;
  const attempt = new Promise<TurnstileApi>((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>("script[data-provider-turnstile], script[src*='challenges.cloudflare.com/turnstile/v0/api.js']");
    const script = existing || document.createElement("script");
    const cleanup = () => {
      clearTimeout(timer);
      script.removeEventListener("load", loaded);
      script.removeEventListener("error", failed);
    };
    const fail = (message: string) => { cleanup(); script.remove(); reject(new Error(message)); };
    const loaded = () => {
      if (window.turnstile) { cleanup(); resolve(window.turnstile); }
      else fail("Cloudflare 验证脚本未能启动，请手动重试。");
    };
    const failed = () => fail("Cloudflare 验证脚本加载失败，请检查网络、内容安全策略及 CDN 脚本优化设置后重试。");
    const timer = setTimeout(() => fail("Cloudflare 验证脚本加载超时，请检查网络及 CDN 设置后重试。"), 15000);
    script.addEventListener("load", loaded, { once: true });
    script.addEventListener("error", failed, { once: true });
    if (!existing) {
      script.dataset.providerTurnstile = "true";
      // Rocket Loader must not defer or rewrite the challenge script.
      script.setAttribute("data-cfasync", "false");
      script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
      script.async = true; script.defer = true;
      document.head.appendChild(script);
    }
  });
  scriptPromise = attempt;
  void attempt.catch(() => { if (scriptPromise === attempt) scriptPromise = null; });
  return attempt;
}

export type ChallengeState = {
  status: "verifying" | "verified" | "error" | "expired" | "consumed";
  message: string;
  code?: string;
};
export function challengeError(code: string, hostname: string): string {
  if (code === "110200") return `上游 Turnstile 未授权当前控制台域名 ${hostname}（110200）。请让服务提供商将此域名加入其 Turnstile 允许列表，或在上游站点登录后使用 Sub2API 令牌连接。本站开启 CDN 或配置自己的 Turnstile 密钥不能代替上游授权。`;
  if (["110100", "110110", "400020", "400021", "400070"].includes(code)) return `上游 Turnstile 站点密钥不可用（${code}），请联系服务提供商检查其配置，或使用 Sub2API 令牌连接。`;
  if (code === "200100") return "验证遇到时钟或缓存问题（200100）。请检查设备时间，并让 CDN 绕过控制台 HTML、/api/* 和 Cloudflare 验证资源的缓存与改写。";
  if (code === "200500") return "验证框无法加载（200500）。请检查网络、广告拦截和内容安全策略是否允许 challenges.cloudflare.com。";
  if (code === "110600" || code === "110620") return `验证超时（${code}），请点击“重新验证”后完成操作。`;
  return `Cloudflare 安全验证未通过${code ? `（${code}）` : ""}。已停止自动刷新，请检查网络后手动重试，或在上游登录后使用令牌连接。`;
}

export type ProviderChallenge = { consume: () => string; dispose: () => void };
export function createProviderChallenge(api: TurnstileApi, element: HTMLElement, siteKey: string, onState: (state: ChallengeState) => void, hostname = window.location.hostname): ProviderChallenge {
  let active = true; let accepting = true; let token = ""; let expiresAt = 0;
  let widgetId: string | null = null; let removeRequested = false;
  const removeWidget = () => {
    removeRequested = true;
    if (widgetId === null) return;
    const id = widgetId; widgetId = null;
    api.remove(id);
  };
  const emit = (state: ChallengeState) => { if (active) onState(state); };
  const stop = (state: ChallengeState) => { if (!active || !accepting) return; accepting = false; token = ""; removeWidget(); emit(state); };
  emit({ status: "verifying", message: "请完成上方安全验证。" });
  widgetId = api.render(element, {
    sitekey: siteKey, retry: "never", "refresh-expired": "manual", "refresh-timeout": "manual", "response-field": false,
    size: element.clientWidth > 0 && element.clientWidth < 300 ? "compact" : "flexible",
    callback: (value) => { if (active && accepting) { token = value; expiresAt = Date.now() + 300000; emit({ status: "verified", message: "验证完成" }); } },
    "error-callback": (code) => { stop({ status: "error", code, message: challengeError(code, hostname) }); },
    "expired-callback": () => stop({ status: "expired", message: "验证已过期，请点击“重新验证”。" }),
    "timeout-callback": () => stop({ status: "expired", message: "验证操作超时，请点击“重新验证”。" }),
    "unsupported-callback": () => stop({ status: "error", message: "当前浏览器不支持此安全验证，请更新浏览器或在上游登录后使用令牌连接。" }),
  });
  // Some integrations invoke an error callback before render returns its id.
  if (removeRequested) removeWidget();
  return {
    consume() {
      if (!active || !accepting || !token) return "";
      if (Date.now() >= expiresAt) { stop({ status: "expired", message: "验证已过期，请点击“重新验证”。" }); return ""; }
      const proof = token; token = ""; accepting = false;
      emit({ status: "consumed", message: "本次验证已用于登录。登录成功后可直接保存；如需再次测试，请重新验证。" });
      return proof;
    },
    dispose() { if (!active) return; active = false; accepting = false; token = ""; removeWidget(); },
  };
}
