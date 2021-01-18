table! {
    version_release(version) {
        version -> Text,
        name -> Text,
        seen -> Bool,
        release_ts -> Timestamp,
        insertion_ts -> Nullable<Timestamp>,
        update_ts -> Nullable<Timestamp>,
    }
}
