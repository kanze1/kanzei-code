//! 移动端桥接设备域(D-386):设备表持久化到 SQLite(state.db mobile_devices 表)。
//!
//! R-270 批1 设备表是纯内存(每次启动清空),本域提供 SQLite 持久化:
//! - 配对成功后写入 device_id + device_token;
//! - 撤销 = 删除行,该 token 立即 401,其它设备不受影响;
//! - 重启后已配对设备仍在(跨进程/跨重启不丢)。

use rusqlite::{params, OptionalExtension};

use super::{SessionStore, StoreError};

impl SessionStore {
    /// 插入/更新一个已配对设备。幂等:同 device_id 覆盖 token/name/paired_at。
    pub fn upsert_mobile_device(
        &self,
        device_id: &str,
        device_token: &str,
        name: &str,
        paired_at_ms: u128,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO mobile_devices(device_id, device_token, name, paired_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id) DO UPDATE SET
                 device_token = excluded.device_token,
                 name = excluded.name,
                 paired_at_ms = excluded.paired_at_ms",
            params![device_id, device_token, name, paired_at_ms as i64],
        )?;
        Ok(())
    }

    /// 全部已配对设备(device_id, device_token, name, paired_at_ms)。
    pub fn list_mobile_devices(&self) -> Result<Vec<(String, String, String, u128)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT device_id, device_token, name, paired_at_ms FROM mobile_devices ORDER BY paired_at_ms",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? as u128,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 撤销一台设备(删除行)。返回是否真的删了(设备不存在 = false)。
    pub fn remove_mobile_device(&self, device_id: &str) -> Result<bool, StoreError> {
        let changed = self.connection.execute(
            "DELETE FROM mobile_devices WHERE device_id = ?1",
            params![device_id],
        )?;
        Ok(changed > 0)
    }

    /// 按 token 查 device_id(认证用;token 被撤销/不存在返回 None)。
    pub fn mobile_device_id_by_token(&self, token: &str) -> Result<Option<String>, StoreError> {
        self.connection
            .query_row(
                "SELECT device_id FROM mobile_devices WHERE device_token = ?1",
                params![token],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// 取全部已配对设备的 token(认证用;R-270 批1 内存表迁移后由本方法接管)。
    pub fn all_mobile_device_tokens(&self) -> Result<Vec<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT device_token FROM mobile_devices")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::testutil::store;

    /// D-386:设备表 CRUD——配对写入、列表读取、撤销删除,重启(重建 store)后仍在。
    #[test]
    fn 设备表持久化_配对列表撤销_重启后仍在() {
        let store = store();
        // 配对:写入两台设备。
        store
            .upsert_mobile_device("dev-1", "tok-a", "手机A", 100)
            .unwrap();
        store
            .upsert_mobile_device("dev-2", "tok-b", "手机B", 200)
            .unwrap();

        // 列表:两台,按 paired_at 升序。
        let list = store.list_mobile_devices().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "dev-1");
        assert_eq!(list[0].2, "手机A");
        assert_eq!(list[0].3, 100);
        assert_eq!(list[1].0, "dev-2");

        // token 反查 + 全部 token。
        assert_eq!(
            store.mobile_device_id_by_token("tok-a").unwrap().as_deref(),
            Some("dev-1")
        );
        assert!(store
            .mobile_device_id_by_token("tok-absent")
            .unwrap()
            .is_none());
        let tokens = store.all_mobile_device_tokens().unwrap();
        assert_eq!(tokens, vec!["tok-a".to_string(), "tok-b".to_string()]);

        // 撤销 dev-1:该 token 立即失效,dev-2 不受影响。
        assert!(store.remove_mobile_device("dev-1").unwrap());
        assert!(!store.remove_mobile_device("dev-1").unwrap());
        assert!(store.mobile_device_id_by_token("tok-a").unwrap().is_none());
        assert_eq!(
            store.mobile_device_id_by_token("tok-b").unwrap().as_deref(),
            Some("dev-2")
        );
        let list = store.list_mobile_devices().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "dev-2");
    }

    /// D-386:upsert 幂等——同 device_id 覆盖 token/name/paired_at,不产生重复行。
    #[test]
    fn 设备upsert幂等_同id覆盖() {
        let store = store();
        store
            .upsert_mobile_device("dev-x", "tok-old", "旧名", 1)
            .unwrap();
        store
            .upsert_mobile_device("dev-x", "tok-new", "新名", 2)
            .unwrap();
        let list = store.list_mobile_devices().unwrap();
        assert_eq!(list.len(), 1, "同 id 覆盖不新增行");
        assert_eq!(list[0].1, "tok-new");
        assert_eq!(list[0].2, "新名");
        assert_eq!(list[0].3, 2);
    }
}
