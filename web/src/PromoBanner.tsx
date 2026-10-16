import { ArrowUpRight, RadioTower } from "lucide-react";
import "./banner.css";

export function PromoBanner() {
  return (
    <aside className="promo-banner" aria-label="杂鱼 API 推广">
      <a href="https://zayuapi.com" target="_blank" rel="noopener noreferrer">
        <span className="promo-brand">
          <RadioTower size={16} aria-hidden="true" />
          <strong>杂鱼 API</strong>
          <span className="promo-label">推广</span>
        </span>
        <span className="promo-description">GPT · Claude · Gemini · Grok</span>
        <span className="promo-destination">
          zayuapi.com
          <ArrowUpRight size={15} aria-hidden="true" />
        </span>
      </a>
    </aside>
  );
}
