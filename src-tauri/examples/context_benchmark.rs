use conch_lib::core::{prepare,Snapshot,Vocabulary};
fn main() {
    let snapshot = Snapshot {vocabulary:(0..50_000).map(|i|Vocabulary {text:format!("maxGroups{i}"),repository:"fixture".into(),file:"fixture.ts".into()}).collect(),..Default::default()};
    let result = prepare("do not change max groups 49999 before 12 tests pass",&snapshot);
    assert_eq!(result.text,result.raw);
    println!("{} entries; {} suggestions",snapshot.vocabulary.len(),result.suggestions.len());
}
