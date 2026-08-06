//! 名字键控、保序、同名 last-wins 的注册表(六注册表的共同底座)。
//! 保序=注入提示词的顺序确定,同名替换原位=覆盖不打乱既有顺序。

pub struct Registry<T> {
    items: Vec<(String, T)>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Registry { items: Vec::new() }
    }
}

impl<T> Registry<T> {
    pub fn insert(&mut self, name: impl Into<String>, item: T) {
        let name = name.into();
        match self.items.iter_mut().find(|(n, _)| *n == name) {
            Some((_, slot)) => *slot = item,
            None => self.items.push((name, item)),
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<T> {
        let pos = self.items.iter().position(|(n, _)| n == name)?;
        Some(self.items.remove(pos).1)
    }

    pub fn get(&self, name: &str) -> Option<&T> {
        self.items.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.items.iter().map(|(n, t)| (n.as_str(), t))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|(n, _)| n.as_str())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_wins_keeps_position() {
        let mut r = Registry::default();
        r.insert("a", 1);
        r.insert("b", 2);
        r.insert("a", 3);
        let collected: Vec<_> = r.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        assert_eq!(collected, vec![("a".to_string(), 3), ("b".to_string(), 2)]);
    }
}
