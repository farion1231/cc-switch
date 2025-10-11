use crate::app_config::AppType;
use crate::provider::{ProviderGroup, SortConfig};
use crate::store::AppState;
use std::collections::HashMap;
use tauri::State;

/// 获取所有分组
#[tauri::command]
pub async fn get_groups(
    state: State<'_, AppState>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<HashMap<String, ProviderGroup>, String> {
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    let groups_config = config.groups_for(&app_type);
    Ok(groups_config.groups.clone())
}

/// 创建分组
#[tauri::command]
pub async fn create_group(
    state: State<'_, AppState>,
    group: serde_json::Value,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<ProviderGroup, String> {
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    // 解析分组基本信息
    let name = group
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("缺少分组名称")?
        .to_string();

    let color = group
        .get("color")
        .and_then(|v| v.as_str())
        .map(String::from);
    let icon = group.get("icon").and_then(|v| v.as_str()).map(String::from);
    let parent_id = group
        .get("parentId")
        .and_then(|v| v.as_str())
        .map(String::from);
    let order = group
        .get("order")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    // 创建新分组
    let mut new_group = ProviderGroup::new(name);
    new_group.color = color;
    new_group.icon = icon;
    new_group.parent_id = parent_id;
    new_group.order = order;

    // 保存到配置
    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    let groups_config = config.groups_for_mut(&app_type);
    groups_config
        .groups
        .insert(new_group.id.clone(), new_group.clone());

    // 如果没有指定顺序，添加到末尾
    if new_group.order.is_none() {
        groups_config.groups_order.push(new_group.id.clone());
    }

    // 保存配置
    config.save()?;

    log::info!("创建分组成功: {} ({})", new_group.name, new_group.id);
    Ok(new_group)
}

/// 更新分组
#[tauri::command]
pub async fn update_group(
    state: State<'_, AppState>,
    group: ProviderGroup,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    let groups_config = config.groups_for_mut(&app_type);

    // 检查分组是否存在
    if !groups_config.groups.contains_key(&group.id) {
        return Err(format!("分组不存在: {}", group.id));
    }

    // 更新时间戳
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut updated_group = group;
    updated_group.updated_at = now;

    // 更新分组
    groups_config
        .groups
        .insert(updated_group.id.clone(), updated_group.clone());

    // 保存配置
    config.save()?;

    log::info!(
        "更新分组成功: {} ({})",
        updated_group.name,
        updated_group.id
    );
    Ok(true)
}

/// 删除分组
#[tauri::command]
pub async fn delete_group(
    state: State<'_, AppState>,
    group_id: String,
    #[allow(non_snake_case)] groupId: Option<String>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let group_id = groupId.unwrap_or(group_id);
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    // 从分组配置中删除
    let groups_config = config.groups_for_mut(&app_type);
    let removed = groups_config.groups.remove(&group_id);

    if removed.is_none() {
        return Err(format!("分组不存在: {}", group_id));
    }

    // 从顺序列表中删除
    groups_config.groups_order.retain(|id| id != &group_id);

    // 将该分组中的供应商移出分组
    if let Some(manager) = config.get_manager_mut(&app_type) {
        for provider in manager.providers.values_mut() {
            if provider.group_id.as_ref() == Some(&group_id) {
                provider.group_id = None;
            }
        }
    }

    // 保存配置
    config.save()?;

    log::info!("删除分组成功: {}", group_id);
    Ok(true)
}

/// 将供应商添加到分组
#[tauri::command]
pub async fn add_provider_to_group(
    state: State<'_, AppState>,
    provider_id: String,
    #[allow(non_snake_case)] providerId: Option<String>,
    group_id: String,
    #[allow(non_snake_case)] groupId: Option<String>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let provider_id = providerId.unwrap_or(provider_id);
    let group_id = groupId.unwrap_or(group_id);
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    // 检查分组是否存在
    let groups_config = config.groups_for_mut(&app_type);
    if !groups_config.groups.contains_key(&group_id) {
        return Err(format!("分组不存在: {}", group_id));
    }

    // 检查供应商是否存在
    let manager = config
        .get_manager_mut(&app_type)
        .ok_or("应用类型配置不存在")?;

    if !manager.providers.contains_key(&provider_id) {
        return Err(format!("供应商不存在: {}", provider_id));
    }

    // 更新供应商的分组ID
    if let Some(provider) = manager.providers.get_mut(&provider_id) {
        provider.group_id = Some(group_id.clone());
    }

    // 将供应商ID添加到分组的列表中（如果不存在）
    let groups_config = config.groups_for_mut(&app_type);
    if let Some(group) = groups_config.groups.get_mut(&group_id) {
        if !group.provider_ids.contains(&provider_id) {
            group.provider_ids.push(provider_id.clone());
            group.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
        }
    }

    // 保存配置
    config.save()?;

    log::info!("将供应商 {} 添加到分组 {}", provider_id, group_id);
    Ok(true)
}

/// 从分组中移除供应商
#[tauri::command]
pub async fn remove_provider_from_group(
    state: State<'_, AppState>,
    provider_id: String,
    #[allow(non_snake_case)] providerId: Option<String>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let provider_id = providerId.unwrap_or(provider_id);
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    // 获取供应商的分组ID
    let manager = config
        .get_manager_mut(&app_type)
        .ok_or("应用类型配置不存在")?;

    let old_group_id = manager
        .providers
        .get(&provider_id)
        .and_then(|p| p.group_id.clone());

    // 清除供应商的分组ID
    if let Some(provider) = manager.providers.get_mut(&provider_id) {
        provider.group_id = None;
    }

    // 从分组的列表中移除供应商ID
    if let Some(old_group_id) = old_group_id {
        let groups_config = config.groups_for_mut(&app_type);
        if let Some(group) = groups_config.groups.get_mut(&old_group_id) {
            group.provider_ids.retain(|id| id != &provider_id);
            group.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
        }
    }

    // 保存配置
    config.save()?;

    log::info!("从分组中移除供应商: {}", provider_id);
    Ok(true)
}

/// 更新分组顺序
#[tauri::command]
pub async fn update_groups_order(
    state: State<'_, AppState>,
    group_ids: Vec<String>,
    #[allow(non_snake_case)] groupIds: Option<Vec<String>>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let group_ids = groupIds.unwrap_or(group_ids);
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    let groups_config = config.groups_for_mut(&app_type);
    groups_config.groups_order = group_ids;

    // 保存配置
    config.save()?;

    log::info!("更新分组顺序成功");
    Ok(true)
}

/// 设置全局排序配置
#[tauri::command]
pub async fn set_global_sort_config(
    state: State<'_, AppState>,
    sort_config: SortConfig,
    #[allow(non_snake_case)] sortConfig: Option<SortConfig>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let sort_config = sortConfig.unwrap_or(sort_config);
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    let groups_config = config.groups_for_mut(&app_type);
    groups_config.global_sort_config = Some(sort_config);

    // 保存配置
    config.save()?;

    log::info!("设置全局排序配置成功");
    Ok(true)
}

/// 设置分组排序配置
#[tauri::command]
pub async fn set_group_sort_config(
    state: State<'_, AppState>,
    group_id: String,
    #[allow(non_snake_case)] groupId: Option<String>,
    sort_config: SortConfig,
    #[allow(non_snake_case)] sortConfig: Option<SortConfig>,
    app_type: Option<AppType>,
    app: Option<AppType>,
) -> Result<bool, String> {
    let group_id = groupId.unwrap_or(group_id);
    let sort_config = sortConfig.unwrap_or(sort_config);
    let app_type = app_type.or(app).unwrap_or(AppType::Claude);

    let mut config = state
        .config
        .lock()
        .map_err(|e| format!("获取配置锁失败: {}", e))?;

    let groups_config = config.groups_for_mut(&app_type);

    // 检查分组是否存在
    if let Some(group) = groups_config.groups.get_mut(&group_id) {
        group.sort_config = Some(sort_config);
        group.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
    } else {
        return Err(format!("分组不存在: {}", group_id));
    }

    // 保存配置
    config.save()?;

    log::info!("设置分组排序配置成功: {}", group_id);
    Ok(true)
}
