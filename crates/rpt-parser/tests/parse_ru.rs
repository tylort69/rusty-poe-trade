//! Russian client texts. Separate test binary because the active language is process-global.

use rpt_parser::*;

#[test]
fn russian_items() {
    rpt_data::init("ru").unwrap();

    let item = parse_clipboard(
        "Класс предмета: Валюта
Редкость: Валюта
Сфера хаоса
--------
Размер стопки: 1 234/5 000
--------
Изменяет редкий предмет, создавая на нём новые случайные свойства
",
    )
    .unwrap();
    assert_eq!(item.category, Some(ItemCategory::Currency));
    assert_eq!(item.info.ref_name, "Chaos Orb");
    assert_eq!(item.stack_size, Some(StackSize { value: 1234.0, max: 5000.0 }));

    let item = parse_clipboard(
        "Класс предмета: Нательные доспехи
Редкость: Редкий
Мрачная оболочка
Великолепные латы
--------
Броня: 850
--------
Уровень предмета: 86
--------
{ Свойство-префикс \"Здоровый\" (Уровень: 8) — Здоровье }
+29(25-29) к максимуму здоровья
{ Свойство-суффикс \"вулкана\" (Уровень: 3) — Стихии, Огонь, Сопротивление }
+40(36-41)% к сопротивлению огню
--------
Осквернено
",
    )
    .unwrap();
    assert_eq!(item.rarity, Some(ItemRarity::Rare));
    assert_eq!(item.category, Some(ItemCategory::BodyArmour));
    assert_eq!(item.info.ref_name, "Glorious Plate");
    assert_eq!(item.armour_ar, Some(850.0));
    assert_eq!(item.item_level, Some(86));
    assert!(item.is_corrupted.is_some());
    assert!(item.unknown_modifiers.is_empty(), "{:#?}", item.unknown_modifiers);
    assert_eq!(item.new_mods[0].info.tier, Some(8));
    assert_eq!(item.new_mods[0].info.generation, Some(ModGeneration::Prefix));
    assert_eq!(item.new_mods[1].info.generation, Some(ModGeneration::Suffix));
    let refs: Vec<_> = item.stats_by_type.iter().map(|c| c.stat.ref_.as_str()).collect();
    assert_eq!(refs, vec!["+# to maximum Life", "+#% to Fire Resistance"]);
}
