//! `AssetRipper.Configuration`: the named configuration data the settings pages edit.
//!
//! The baseline keeps two keyed stores on the configuration object: `SingletonData`, one text per
//! key, and `ListData`, a list of texts per key. The settings pages read and write them. The three
//! Rust settings records also have linked typed singleton entries so both settings pages stay in
//! sync.
//!
//! **The two stores are not persisted.** `CoreConfiguration.ResetToDefaultValues` clears them, and
//! nothing in `SerializedSettings` carries them, so they are in-memory configuration for the session
//! rather than part of the settings file. That is why this module has no loader and no writer.
//!
//! Keys retain insertion order. The frozen .NET 10 runtime exposes the `Dictionary` insertion order
//! on the configuration page, and the stores add default entries and user-created keys in that
//! order.

/// `SingletonDataStorage`: one text per key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SingletonData {
    entries: Vec<(String, String)>,
}

impl SingletonData {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(key, _)| key)
    }

    /// The text stored for `key`, when the key exists.
    pub fn get(&self, key: &str) -> Option<&String> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value)
    }

    /// The number of stored keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no keys.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The text for `key`, adding an empty entry when the key is absent.
    ///
    /// The baseline's `GetOrAdd` adds a `StringDataInstance`, whose value is the empty string, so
    /// the key appears with no data - which is what the page shows as "nothing loaded for this key".
    pub fn get_or_add(&mut self, key: &str) -> &mut String {
        let index = match self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)
        {
            Some(index) => index,
            None => {
                self.entries.push((key.to_owned(), String::new()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[index].1
    }

    /// `DataInstance.Clear`: resets the value to empty, **keeping the key**.
    ///
    /// The page's Remove button calls this rather than removing the entry, so a removed key stays
    /// listed with no data. Removing it would change what the page shows.
    pub fn clear(&mut self, key: &str) -> bool {
        match self
            .entries
            .iter_mut()
            .find(|(candidate, _)| candidate == key)
        {
            Some((_, value)) => {
                value.clear();
                true
            }
            None => false,
        }
    }

    /// `DataStorage.Clear`: resets every value, keeping every key.
    pub fn clear_all(&mut self) {
        for (_, value) in &mut self.entries {
            value.clear();
        }
    }
}

/// `ListDataStorage`: a list of texts per key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListData {
    entries: Vec<(String, Vec<String>)>,
}

impl ListData {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(key, _)| key)
    }

    /// The list stored for `key`, when the key exists.
    pub fn get(&self, key: &str) -> Option<&Vec<String>> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, values)| values)
    }

    /// The number of stored keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no keys.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The list for `key`, adding an empty list when the key is absent.
    pub fn get_or_add(&mut self, key: &str) -> &mut Vec<String> {
        let index = match self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == key)
        {
            Some(index) => index,
            None => {
                self.entries.push((key.to_owned(), Vec::new()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[index].1
    }

    /// Appends the values to `key`'s list, adding the key when it is absent.
    ///
    /// `DataSet.StringAccessor.AddRange` appends; the page's add form appends to an existing list
    /// rather than replacing it, which is why the button is a plain `+` on a non-empty list.
    pub fn add_range<I, S>(&mut self, key: &str, values: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let list = self.get_or_add(key);
        list.extend(values.into_iter().map(Into::into));
    }

    /// Whether `index` addresses an element, mirroring `DataExtensions.ContainsIndex`.
    pub fn contains_index(&self, key: &str, index: i64) -> bool {
        match self.get(key) {
            Some(list) => usize::try_from(index).is_ok_and(|index| index < list.len()),
            None => false,
        }
    }

    /// `DataSet.RemoveAt`: removes the element at `index`, keeping the key.
    pub fn remove_at(&mut self, key: &str, index: i64) -> bool {
        let Some((_, list)) = self
            .entries
            .iter_mut()
            .find(|(candidate, _)| candidate == key)
        else {
            return false;
        };
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        if index >= list.len() {
            return false;
        }
        list.remove(index);
        true
    }

    /// `StringAccessor[index] = value`: replaces the element at `index`.
    pub fn replace_at(&mut self, key: &str, index: i64, content: String) -> bool {
        let Some((_, list)) = self
            .entries
            .iter_mut()
            .find(|(candidate, _)| candidate == key)
        else {
            return false;
        };
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        match list.get_mut(index) {
            Some(value) => {
                *value = content;
                true
            }
            None => false,
        }
    }

    /// `DataStorage.Clear`: empties every list, keeping every key.
    pub fn clear_all(&mut self) {
        for (_, list) in &mut self.entries {
            list.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ListData, SingletonData};

    /// Clearing a singleton resets its value and keeps its key.
    #[test]
    fn clearing_a_singleton_keeps_the_key() {
        let mut data = SingletonData::new();
        data.get_or_add("README").push_str("some text");
        assert_eq!(data.get("README"), Some(&"some text".to_owned()));

        assert!(data.clear("README"));
        assert_eq!(
            data.get("README"),
            Some(&String::new()),
            "the key stays, with no data"
        );
        assert_eq!(data.keys().count(), 1);
        // An absent key is reported rather than added.
        assert!(!data.clear("missing"));
        assert_eq!(data.keys().count(), 1);
    }

    /// `GetOrAdd` adds an empty entry, which is what the page shows as nothing loaded.
    #[test]
    fn get_or_add_adds_an_empty_entry() {
        let mut data = SingletonData::new();
        assert_eq!(data.get_or_add("KEY"), &String::new());
        assert_eq!(data.get("KEY"), Some(&String::new()));
        assert!(data.contains_key_for_test("KEY"));
    }

    /// `DataStorage.Clear` resets every value and removes no key.
    #[test]
    fn clearing_all_keeps_every_key() {
        let mut singletons = SingletonData::new();
        singletons.get_or_add("A").push('a');
        singletons.get_or_add("B").push('b');
        singletons.clear_all();
        assert_eq!(singletons.keys().count(), 2);
        assert_eq!(singletons.get("A"), Some(&String::new()));
        assert_eq!(singletons.get("B"), Some(&String::new()));

        let mut lists = ListData::new();
        lists.add_range("L", ["one", "two"]);
        lists.clear_all();
        assert_eq!(lists.keys().count(), 1);
        assert_eq!(lists.get("L"), Some(&Vec::new()));
    }

    /// A list appends, removes and replaces by index, and reports an out-of-range index.
    #[test]
    fn a_list_appends_removes_and_replaces_by_index() {
        let mut data = ListData::new();
        data.add_range("L", ["a", "b"]);
        // Appending to an existing key extends rather than replaces.
        data.add_range("L", ["c"]);
        assert_eq!(
            data.get("L"),
            Some(&vec!["a".to_owned(), "b".to_owned(), "c".to_owned()])
        );

        assert!(data.contains_index("L", 2));
        assert!(!data.contains_index("L", 3));
        assert!(!data.contains_index("L", -1));

        assert!(data.replace_at("L", 1, "B".to_owned()));
        assert_eq!(
            data.get("L"),
            Some(&vec!["a".to_owned(), "B".to_owned(), "c".to_owned()])
        );
        assert!(!data.replace_at("L", 3, "d".to_owned()));
        assert!(!data.replace_at("missing", 0, "d".to_owned()));

        assert!(data.remove_at("L", 0));
        assert_eq!(data.get("L"), Some(&vec!["B".to_owned(), "c".to_owned()]));
        assert!(!data.remove_at("L", 5));
        assert!(!data.remove_at("missing", 0));
        // The key survives every mutation, as `DataSet` does.
        assert_eq!(data.keys().count(), 1);
    }

    /// The stores retain the insertion order exposed by the source dictionary.
    #[test]
    fn the_keys_keep_insertion_order() {
        let mut data = SingletonData::new();
        for key in ["zeta", "alpha", "mu"] {
            data.get_or_add(key);
        }
        let keys: Vec<&String> = data.keys().collect();
        assert_eq!(keys, vec!["zeta", "alpha", "mu"]);
        data.get_or_add("alpha").push_str("updated");
        assert_eq!(
            data.keys().map(String::as_str).collect::<Vec<_>>(),
            ["zeta", "alpha", "mu"]
        );

        let mut lists = ListData::new();
        lists.add_range("second", ["2"]);
        lists.add_range("first", ["1"]);
        assert_eq!(
            lists.keys().map(String::as_str).collect::<Vec<_>>(),
            ["second", "first"]
        );
    }

    impl SingletonData {
        /// A test-only key check, since the store exposes no membership method.
        fn contains_key_for_test(&self, key: &str) -> bool {
            self.get(key).is_some()
        }
    }
}
