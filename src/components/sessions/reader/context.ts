import { createContext, useContext } from "react";
import { AGENT_READER_STYLES, type AgentReaderStyle } from "./agentStyles";

/**
 * 阅读页各行组件共用的会话级信息。经 Context 下发，避免每一行都透传一长串 props；
 * 值在 SessionReader 里 useMemo，只有会话、查找词变化时才更新。
 */
export interface ReaderContextValue {
  providerId?: string;
  sourcePath?: string;
  /** 会话项目目录：路径显示为相对路径 */
  projectDir?: string;
  style: AgentReaderStyle;
  /** 助手名（Claude Code / Codex …） */
  appName: string;
  /** 当前高亮词：查找框打开时是查找词，否则是列表搜索词 */
  searchQuery?: string;
  onCopy: (text: string, message: string) => void;
}

export const ReaderContext = createContext<ReaderContextValue>({
  style: AGENT_READER_STYLES.generic,
  appName: "",
  onCopy: () => undefined,
});

export const useReaderContext = () => useContext(ReaderContext);
