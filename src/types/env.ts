/**
 * 环境变量冲突检测相关类型定义
 */

/**
 * 环境变量冲突信息
 */
export interface EnvConflict {
  /** 环境变量名称 */
  varName: string;
  /** 环境变量的值 */
  varValue: string;
  /** 来源类型: "system" 表示系统环境变量, "file" 表示配置文件 */
  sourceType: "system" | "file";
  /** 来源路径 (注册表路径或文件路径:行号) */
  sourcePath: string;
}

/**
 * 备份信息
 */
export interface BackupInfo {
  /** 备份文件路径 */
  backupPath: string;
  /** 备份时间戳 */
  timestamp: string;
  /** 被备份的环境变量冲突列表 */
  conflicts: EnvConflict[];
}
