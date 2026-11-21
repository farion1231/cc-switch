interface IconProps {
  size?: number;
  className?: string;
}

// 导入本地 SVG 图标
import ClaudeSvg from "@/icons/extracted/claude.svg?url";
import OpenAISvg from "@/icons/extracted/openai.svg?url";
import GeminiSvg from "@/icons/extracted/gemini.svg?url";
import OpenClawSvg from "@/icons/extracted/claw.svg?url";

export function ClaudeIcon({ size = 16, className = "" }: IconProps) {
  return (
    <img
      src={ClaudeSvg}
      width={size}
      height={size}
      className={className}
      alt="Claude"
      loading="lazy"
    />
  );
}

export function CodexIcon({ size = 16, className = "" }: IconProps) {
  return (
    <img
      src={OpenAISvg}
      width={size}
      height={size}
      className={`dark:brightness-0 dark:invert ${className}`}
      alt="Codex"
      loading="lazy"
    />
  );
}

export function GeminiIcon({ size = 16, className = "" }: IconProps) {
  return (
    <img
      src={GeminiSvg}
      width={size}
      height={size}
      className={className}
      alt="Gemini"
      loading="lazy"
    />
  );
}

export function OpenClawIcon({ size = 16, className = "" }: IconProps) {
  return (
    <img
      src={OpenClawSvg}
      width={size}
      height={size}
      className={className}
      alt="OpenClaw"
      loading="lazy"
    />
  );
}

// Claude Desktop-style Skills scroll, based on the reference image.
export function SkillsIcon({ size = 16, className = "" }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
      focusable="false"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path d="M4 3h14a2 2 0 0 1 2 2v12" />
      <path d="M2 7V5a2 2 0 0 1 4 0v14a2 2 0 0 0 4 0v-2h12v2a2 2 0 0 1-2 2H8" />
      <path d="m9 10.5 1.5-2-.5 2 2-1.5-.5 1.5 2-1.5-.5 1.5 2-2" />
    </svg>
  );
}
