use crate::parser::*;
use crate::parser::test_support::*;
use crate::ParseLanguage;

#[test]
fn byte_prefixes_are_explicit_unambiguous_and_comparable() {
    let amount = |text| first_measurement(text).measurement_fact().expect("measurement fact").values()[0].amount();
    assert_eq!(amount("1 MB"), amount("1000 kB"));
    assert_eq!(amount("1 GB"), amount("1000 MB"));
    assert_eq!(amount("1 PB"), amount("1000 TB"));
    assert_eq!(amount("1 KB"), amount("1 kB"));
    assert_eq!(first_measurement("1 KB/s").measurement_fact().expect("measurement fact").unit(), "kB/s");
    assert!(!parse_nodes(&crate::tokenized_text_tree("1 mB"), ParseLanguage::English).iter().any(|node| node.kind() == NodeKind::Measurement));
    for text in ["1 dB", "1 dBA"] {
        let measurement = first_measurement(text);
        let fact = measurement.measurement_fact().expect("measurement fact");
        assert_eq!(fact.values()[0].quantity_kind(), MeasurementQuantityKind::LogarithmicLevel);
        assert_eq!(fact.values()[0].amount(), NORMALIZED_VALUE_SCALE as i128);
    }
    assert!(unit_symbols_are_unique());
    assert!(unit_semantics_are_complete());
}


#[test]
fn detects_numbers_dates_and_measurements() {
    let units = vec![
        u(UnitKind::Numeric, "1", 1),
        u(UnitKind::Punct, ".", 1),
        u(UnitKind::Numeric, "234", 3),
        u(UnitKind::Punct, ",", 1),
        u(UnitKind::Numeric, "56", 2),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Numeric, "2024", 4),
        u(UnitKind::Punct, "-", 1),
        u(UnitKind::Numeric, "06", 2),
        u(UnitKind::Punct, "-", 1),
        u(UnitKind::Numeric, "29", 2),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Numeric, "10", 2),
        u(UnitKind::Punct, ",", 1),
        u(UnitKind::Numeric, "5", 1),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "kg", 2),
    ];
    let nodes = parse_nodes(&stream(units, "1.234,56 2024-06-29 10,5 kg"), ParseLanguage::Neutral);
    let number = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Number)
        .expect("number");
    let date = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Date)
        .expect("date");
    let measurement = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Measurement)
        .expect("measurement");
    assert!(number.iter_children().next().is_none());
    assert!(date.iter_children().next().is_none());
    assert!(measurement.iter_children().next().is_none());
    assert_eq!(number.number_values(), &[12_345_600_000_000]);
    assert_eq!(date.date_fact(), Some(DateFact::new(2024, 6, 29)));
}


#[test]
fn glued_dimension_without_unit_is_unnormalized_number_phrase() {
    let tree = crate::tokenized_text_tree("600×400");
    let context = ParseContext::for_language(&tree, ParseLanguage::Neutral, &[]);
    let token = BoundedToken::new(&tree, 0).expect("bounded token");
    let phrase = parse_number_phrase_at(&context, 0, &token).expect("dimension number phrase");
    let detected = phrase.resolve();
    let node = materialize_detected(&tree, detected);
    assert_eq!(node.kind(), NodeKind::Number);
    assert!(node.semantic().is_none());
}


#[test]
fn detects_measurements_with_direct_units() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("-150°"), ParseLanguage::Neutral);
    assert_eq!(nodes.first().map(|node| node.kind()), Some(NodeKind::Measurement));

    let nodes = parse_nodes(&crate::tokenized_text_tree("20°C"), ParseLanguage::Neutral);
    assert_eq!(nodes.first().map(|node| node.kind()), Some(NodeKind::Measurement));

    let nodes = parse_nodes(&crate::tokenized_text_tree("150°"), ParseLanguage::Neutral);
    assert_eq!(nodes.first().map(|node| node.kind()), Some(NodeKind::Measurement));
    let nodes = parse_nodes(&crate::tokenized_text_tree(&format!("150\u{FE0F}")), ParseLanguage::Neutral);
    assert_eq!(nodes.first().map(|node| node.kind()), Some(NodeKind::Measurement));

    let nodes = parse_nodes(&crate::tokenized_text_tree("20 °"), ParseLanguage::Neutral);
    assert_eq!(nodes.first().map(|node| node.kind()), Some(NodeKind::Measurement));

    let nodes = parse_nodes(&crate::tokenized_text_tree("20 °C"), ParseLanguage::Neutral);
    assert_eq!(nodes.first().map(|node| node.kind()), Some(NodeKind::Measurement));

    let nodes = parse_nodes(&crate::tokenized_text_tree("20°C/68°F"), ParseLanguage::Neutral);
    let kinds: Vec<NodeKind> = nodes.iter().map(|node| node.kind()).collect();
    assert_eq!(kinds, vec![NodeKind::Measurement, NodeKind::Punct, NodeKind::Measurement]);
}


#[test]
fn detects_beats_per_minute_as_frequency_measurement() {
    for text in ["1 bpm", "60bpm"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let measurement = nodes.iter().find(|node| node.kind() == NodeKind::Measurement).expect("bpm measurement");
        assert_eq!(&text[measurement.span()], text);
        assert_eq!(measurement.measurement_fact().expect("measurement fact").unit(), "bpm");
    }

    let nodes = parse_nodes(&crate::tokenized_text_tree("60 bpm"), ParseLanguage::Neutral);
    let value = nodes.iter().find_map(|node| node.measurement_fact()).and_then(|fact| fact.values().first().copied()).expect("frequency value");
    assert_eq!(value.dimension(), MeasurementDimension::Frequency);
    assert_eq!(value.amount(), NORMALIZED_VALUE_SCALE as i128);

    for text in ["1 BPM", "1 Bpm"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Measurement), "text {text}");
    }
}


#[test]
fn does_not_detect_removed_opaque_memoq_measurement_suffixes() {
    for text in ["KKV", "M/L", "Onm", "V/EN", "V/max", "g/os", "kV/IT", "ml/hod"]
        .into_iter()
        .flat_map(|unit| [format!("7{unit}"), format!("7 {unit}")])
    {
        let nodes = parse_nodes(&crate::tokenized_text_tree(&text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Measurement), "text {text}");
    }
}


#[test]
fn detects_semantic_composite_measurement_suffixes_with_spaces() {
    for text in ["5 mg/ml", "12 W/m²", "8 µS/cm", "4 km/h", "6 N/mm2", "30 Mbit/s"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(nodes.iter().any(|node| node.kind() == NodeKind::Measurement && node.span() == (0..text.len())), "text {text}");
    }
}


#[test]
fn compound_units_normalize_through_structural_dimensions() {
    let normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .expect("normalized measurement")
    };

    assert_eq!(normalized("1 m/s"), normalized("3.6 km/h"));
    assert_eq!(normalized("1 m²"), normalized("10000 cm²"));
    assert_eq!(normalized("1 mg/l"), normalized("1 g/m³"));
    assert_eq!(normalized("1 N/mm²"), normalized("1 MPa"));
    assert_eq!(normalized("1 µL/min"), normalized("1 mm³/min"));
}


#[test]
fn recommended_measurement_units_normalize_to_existing_dimensions() {
    let normalized = |text: &str, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    for (source, target) in [
        ("1 ha", "10000 m²"),
        ("1 acre", "4046.8564224 m²"),
        ("1 fur", "201.168m"),
        ("1 t", "1000 kg"),
        ("1 cal", "4.184 J"),
        ("1 kcal", "4184 J"),
        ("1 mmHg", "133.322387415 Pa"),
        ("1 mph", "0.44704 m/s"),
        ("60 rpm", "1 Hz"),
        ("1 PS", "735.49875W"),
        ("1 HP", "745.699871582W"),
        ("1C", "1 A·s"),
        ("1F", "1 A·s/V"),
        ("1H", "1 V·s/A"),
        ("1Wb", "1 V·s"),
        ("1T", "1 Wb/m²"),
    ] {
        let source = normalized(source, ParseLanguage::English);
        let target = normalized(target, ParseLanguage::English);
        assert_eq!(source.dimension(), target.dimension());
        assert_eq!(source.quantity_kind(), target.quantity_kind());
        assert!(source.amount().abs_diff(target.amount()) <= 1, "{source:?} and {target:?}");
    }

    assert_eq!(normalized("1 mil", ParseLanguage::Swedish), normalized("10 km", ParseLanguage::Swedish));
    assert_ne!(normalized("1 mil", ParseLanguage::Swedish), normalized("1 mi", ParseLanguage::Swedish));
    assert_eq!(normalized("1 tum", ParseLanguage::Swedish), normalized("2.54 cm", ParseLanguage::English));
    assert_eq!(normalized("1 Zoll", ParseLanguage::German), normalized("2.54 cm", ParseLanguage::English));
    assert_eq!(normalized("1 hästkraft", ParseLanguage::Swedish), normalized("735498750 µW", ParseLanguage::English));
    assert_eq!(normalized("1 Pferdestärke", ParseLanguage::German), normalized("735498750 µW", ParseLanguage::English));
    let horsepower = normalized("1 horsepower", ParseLanguage::English);
    let nanowatts = normalized("745699871582 nW", ParseLanguage::English);
    assert_eq!(horsepower.dimension(), nanowatts.dimension());
    assert_eq!(horsepower.quantity_kind(), nanowatts.quantity_kind());
    assert!(horsepower.amount().abs_diff(nanowatts.amount()) <= 1);
}


#[test]
fn additional_recommended_units_have_exact_typed_semantics() {
    let measurement = |text: &str, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| (fact.unit().to_string(), fact.values().first().copied())))
            .unwrap_or_else(|| panic!("measurement for {text:?}"))
    };
    let value = |text: &str, language| measurement(text, language).1.expect("normalized measurement value");

    for (source, target) in [
        ("1 atm", "101325 Pa"),
        ("760 Torr", "1 atm"),
        ("1 Å", "0.1 nm"),
        ("1 cP", "0.001 Pa·s"),
        ("1 cSt", "1 mm²/s"),
    ] {
        assert_eq!(value(source, ParseLanguage::English), value(target, ParseLanguage::English), "{source:?} and {target:?}");
    }

    for (text, language, canonical) in [
        ("1 atmosphere", ParseLanguage::English, "atm"),
        ("1 atmosfär", ParseLanguage::Swedish, "atm"),
        ("1 Atmosphäre", ParseLanguage::German, "atm"),
        ("1 ångström", ParseLanguage::Swedish, "Å"),
        ("1 Angström", ParseLanguage::German, "Å"),
        ("1 centipoise", ParseLanguage::English, "cP"),
        ("1 Centistokes", ParseLanguage::German, "cSt"),
    ] {
        assert_eq!(measurement(text, language).0, canonical, "localized unit {text:?}");
    }

    assert_eq!(value("1 atm", ParseLanguage::English).quantity_kind(), MeasurementQuantityKind::Pressure);
    assert_eq!(value("1 cP", ParseLanguage::English).quantity_kind(), MeasurementQuantityKind::DynamicViscosity);
    assert_eq!(value("1 cSt", ParseLanguage::English).quantity_kind(), MeasurementQuantityKind::KinematicViscosity);
}


#[test]
fn distinctive_scientific_and_pressure_units_have_typed_semantics() {
    let measurement = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| (fact.unit().to_string(), fact.values().first().copied())))
            .unwrap_or_else(|| panic!("measurement for {text:?}"))
    };
    let value = |text: &str| measurement(text).1.unwrap_or_else(|| panic!("normalized value for {text:?}"));

    for (source, target) in [
        ("1 Ci", "37 GBq"),
        ("1 dyn", "10 µN"),
        ("1 erg", "0.1 µJ"),
        ("1 Mx", "10 nWb"),
        ("1 inHg", "3386.389 Pa"),
        ("1 mmH2O", "9.80665 Pa"),
        ("1 cmH₂O", "98.0665 Pa"),
        ("1 inH₂O", "249.08891 Pa"),
    ] {
        assert_eq!(value(source), value(target), "{source:?} and {target:?}");
    }

    let oersted = value("1 Oe");
    let amperes_per_metre = value("79.5774715459 A/m");
    assert_eq!(oersted.dimension(), amperes_per_metre.dimension());
    assert_eq!(oersted.quantity_kind(), MeasurementQuantityKind::MagneticFieldStrength);
    assert!(oersted.amount().abs_diff(amperes_per_metre.amount()) <= 1);

    assert_eq!(measurement("1 Jy").0, "Jy");
    assert_eq!(resolve_measurement_unit("Jy", ParseLanguage::English).map(|unit| unit.quantity_kind()), Some(MeasurementQuantityKind::SpectralFluxDensity));
    assert_eq!(value("1 mEq").quantity_kind(), MeasurementQuantityKind::ChemicalEquivalentAmount);
    assert_eq!(value("1 mEq/l").quantity_kind(), MeasurementQuantityKind::ChemicalEquivalentConcentration);
    assert_eq!(value("1 mOsm").quantity_kind(), MeasurementQuantityKind::OsmoticAmount);
    assert_eq!(value("1 mOsm/l").quantity_kind(), MeasurementQuantityKind::OsmoticConcentration);
    assert_ne!(value("1 mEq"), value("1 mmol"));
    assert_ne!(value("1 mOsm"), value("1 mmol"));
    assert_eq!(value("1 Gs").quantity_kind(), MeasurementQuantityKind::Time);
}


#[test]
fn physical_and_data_suffixes_have_semantics() {
    let normalized = |text: &str, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    for (source, target) in [
        ("1 Wh", "3600 J"),
        ("1 kWh", "3.6 MJ"),
        ("1 MWh", "3.6 GJ"),
        ("1 Ah", "3600C"),
        ("1 mAh", "3.6C"),
        ("1 mAs", "0.001C"),
        ("60 U/min", "1 Hz"),
        ("1 qm", "1 m²"),
        ("1 mg/qm", "1 mg/m²"),
        ("1 KWh/m", "3.6 MJ/m"),
        ("1 Gbyte", "1 GB"),
        ("1 Tbyte", "1 TB"),
        ("1 Pbyte", "1 PB"),
        ("8 kbps", "1 kB/s"),
        ("1 Kbit/s", "1 kbit/s"),
        ("1 Mbit/Sek", "1 Mbit/s"),
        ("1 Gb/s", "1 Gbit/s"),
        ("1 Tb/s", "1 Tbit/s"),
        ("8 B/s", "64 bit/s"),
    ] {
        let language = if matches!(source, "60 U/min" | "1 qm" | "1 mg/qm" | "1 Mbit/Sek") { ParseLanguage::German } else { ParseLanguage::English };
        assert_eq!(normalized(source, language), normalized(target, language), "{source:?} and {target:?}");
    }
}


#[test]
fn pico_time_grain_angle_and_ratio_suffixes_have_semantics() {
    let normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    for (source, target) in [
        ("1000 pF", "1 nF"),
        ("1 pmol/L", "1 nmol/m³"),
        ("1d", "24 h"),
        ("1a", "365.25d"),
        ("1 mm/hr", "1 mm/h"),
        ("24 mg/d", "1 mg/h"),
        ("1 °K/h", "1 K/h"),
        ("1 gr", "0.06479891g"),
        ("1°", "60′"),
        ("1°", "3600″"),
        ("1°", "216000‴"),
        ("1°/s", "60′/s"),
        ("1%", "10000 ppm"),
    ] {
        assert_eq!(normalized(source), normalized(target), "{source:?} and {target:?}");
    }

    let german_normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::German)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized German measurement for {text:?}"))
    };
    assert_eq!(german_normalized("1 h/Tag"), german_normalized("1 h/d"));

}


#[test]
fn electrical_power_and_radiation_units_preserve_quantity_meaning() {
    let normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    for (source, target) in [
        ("1 kVA", "1000 VA"),
        ("1 MVA", "1000 kVA"),
        ("1 kVAr", "1000 VAr"),
        ("1 MWe", "1 MW"),
        ("1 Gy", "1000 mGy"),
        ("1 Sv", "1000 mSv"),
        ("3.6 mGy/h", "1 µGy/s"),
        ("3.6 mSv/h", "1 µSv/s"),
        ("3.6 mR/h", "1 µR/s"),
        ("1R", "0.000258 C/kg"),
    ] {
        assert_eq!(normalized(source), normalized(target), "{source:?} and {target:?}");
    }

    assert_ne!(normalized("1 VA"), normalized("1W"));
    assert_ne!(normalized("1 VAr"), normalized("1W"));
    assert_ne!(normalized("1 Gy"), normalized("1 Sv"));
}


#[test]
fn standards_gap_units_have_typed_semantics_and_conservative_ambiguity() {
    let value = |text: &str, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
    };
    let unit = |text: &str, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()))
    };

    for (source, target) in [
        ("72pt", "1 in"),
        ("1 nmi", "1852m"),
        ("1 kn", "1 nmi/h"),
        ("1 psi", "6894.757293168 Pa"),
        ("1 rem", "0.01 Sv"),
    ] {
        assert_eq!(value(source, ParseLanguage::English), value(target, ParseLanguage::English), "{source:?} and {target:?}");
    }

    assert_ne!(value("1 Bq", ParseLanguage::English), value("1 Hz", ParseLanguage::English));
    assert_ne!(value("1 cd", ParseLanguage::English), value("1 lm", ParseLanguage::English));
    assert_eq!(value("1 eV", ParseLanguage::English), None);
    assert_eq!(value("1 Da", ParseLanguage::English), None);
    assert!(value("1 GeV", ParseLanguage::English).is_some());
    assert!(value("1 PDa", ParseLanguage::English).is_some());

    assert_eq!(unit("1rem", ParseLanguage::English), None);
    assert_eq!(unit("1 rem", ParseLanguage::English).as_deref(), Some("rem"));
    assert_eq!(unit("1mrem", ParseLanguage::English).as_deref(), Some("mrem"));
    assert_eq!(unit("12 pt", ParseLanguage::English), None);
    assert_eq!(unit("12pt", ParseLanguage::English).as_deref(), Some("pt"));
    assert_eq!(unit("5 rad", ParseLanguage::Swedish), None);
    assert_eq!(unit("5rad", ParseLanguage::Swedish).as_deref(), Some("rad"));
    assert_eq!(unit("5 rad", ParseLanguage::English).as_deref(), Some("rad"));
}


#[test]
fn trados_registry_unit_gaps_have_typed_conservative_coverage() {
    let measurement = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find(|node| node.kind() == NodeKind::Measurement)
    };
    let value = |text: &str| measurement(text).and_then(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()));
    let unit = |text: &str| measurement(text).and_then(|node| node.measurement_fact().map(|fact| fact.unit().to_string()));

    assert_eq!(value("1 au"), value("149597870700m"));
    assert_eq!(value("1 ua"), value("1 au"));
    assert_eq!(value("1 short cwt"), value("100lb"));
    assert_eq!(value("1 long cwt"), value("112lb"));
    assert_eq!(value("1 oz avdp"), value("1oz"));
    assert_eq!(value("1 lb avdp"), value("1lb"));
    assert_eq!(value("1 sq in"), value("1in²"));
    assert_eq!(value("1 sq ft"), value("1ft²"));
    assert_eq!(value("1 sq yd"), value("1yd²"));
    assert_eq!(value("1 sq mi"), value("1mi²"));

    assert_eq!(unit("1u").as_deref(), Some("Da"));
    assert_eq!(unit("1 u"), None);
    assert_eq!(unit("1ua").as_deref(), Some("au"));
    assert_eq!(unit("1 kat").as_deref(), Some("kat"));
    assert_eq!(value("1 kat").map(|fact| fact.quantity_kind()), Some(MeasurementQuantityKind::CatalyticActivity));
    assert_eq!(value("1 Np").map(|fact| fact.quantity_kind()), Some(MeasurementQuantityKind::NeperLevel));
    assert_ne!(value("1 Np"), value("1 dB"));

    for (source, canonical) in [
        ("1 KV", "kV"), ("1 KA", "kA"), ("1 KJ", "kJ"), ("1 KN", "kN"), ("1 KW", "kW"), ("1 KK", "kK"), ("1 KPS", "kPS"),
        ("1 Nm", "N·m"), ("1 mNm", "mN·m"), ("1 MNm", "MN·m"), ("1 kNm", "kN·m"), ("1 KNm", "kN·m"), ("1 GNm", "GN·m"),
    ] {
        assert_eq!(unit(source).as_deref(), Some(canonical), "source {source:?}");
    }

    assert_eq!(unit("1GPS"), Some("GPS".to_string()));
    assert_eq!(unit("1 GPS"), None);
}


#[test]
fn trados_derived_units_preserve_specific_quantity_kinds() {
    let parsed = |text: &str| {
        let measurement = first_measurement(text);
        let fact = measurement.measurement_fact().expect("measurement fact");
        (fact.unit().to_string(), fact.values().first().map(MeasurementValueFact::quantity_kind))
    };

    for (text, canonical, expected) in [
        ("1 Nm", "N·m", MeasurementQuantityKind::Torque),
        ("1 Pa s", "Pa·s", MeasurementQuantityKind::DynamicViscosity),
        ("1 N/m", "N/m", MeasurementQuantityKind::ForcePerLength),
        ("1 rad/s²", "rad/s²", MeasurementQuantityKind::AngularAcceleration),
        ("1 W/m²", "W/m²", MeasurementQuantityKind::PowerPerArea),
        ("1 J/K", "J/K", MeasurementQuantityKind::HeatCapacity),
        ("1 J/(kg K)", "J/kg·K", MeasurementQuantityKind::SpecificHeatCapacity),
        ("1 J/kg", "J/kg", MeasurementQuantityKind::SpecificEnergy),
        ("1 W/(m K)", "W/m·K", MeasurementQuantityKind::ThermalConductivity),
        ("1 J/m³", "J/m³", MeasurementQuantityKind::EnergyDensity),
        ("1 V/m", "V/m", MeasurementQuantityKind::ElectricFieldStrength),
        ("1 C/m³", "C/m³", MeasurementQuantityKind::VolumeChargeDensity),
        ("1 C/m²", "C/m²", MeasurementQuantityKind::SurfaceChargeDensity),
        ("1 F/m", "F/m", MeasurementQuantityKind::Permittivity),
        ("1 H/m", "H/m", MeasurementQuantityKind::Permeability),
        ("1 J/mol", "J/mol", MeasurementQuantityKind::MolarEnergy),
        ("1 J/(mol K)", "J/mol·K", MeasurementQuantityKind::MolarHeatCapacity),
        ("1 W/sr", "W/sr", MeasurementQuantityKind::RadiantIntensity),
        ("1 W/(m² sr)", "W/m²·sr", MeasurementQuantityKind::Radiance),
        ("1 kat/m³", "kat/m³", MeasurementQuantityKind::CatalyticActivityConcentration),
        ("1 m³/kg", "m³/kg", MeasurementQuantityKind::SpecificVolume),
        ("1 A/m²", "A/m²", MeasurementQuantityKind::ElectricCurrentDensity),
        ("1 A/m", "A/m", MeasurementQuantityKind::MagneticFieldStrength),
        ("1 cd/m²", "cd/m²", MeasurementQuantityKind::Luminance),
        ("1 g/kg", "g/kg", MeasurementQuantityKind::MassFraction),
        ("1 g/m²", "g/m²", MeasurementQuantityKind::ArealMassDensity),
    ] {
        assert_eq!(parsed(text), (canonical.to_string(), Some(expected)), "text {text:?}");
    }
}


#[test]
fn specialized_measurement_suffixes_have_specific_semantics() {
    let normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    assert_eq!(normalized("60 Nl/min"), normalized("1 Nl/s"));
    assert_eq!(normalized("1 f/s"), normalized("1 fr/s"));
    assert_eq!(normalized("1000 GFLOPS"), normalized("1 TFLOPS"));
    assert_eq!(normalized("1 gigaflops"), normalized("1 GFLOPS"));
    assert_eq!(normalized("60 ct/min").amount(), NORMALIZED_VALUE_SCALE as i128);
    assert_eq!(normalized("1 LP/mm").amount(), 1_000 * NORMALIZED_VALUE_SCALE as i128);
    assert_eq!(normalized("1 pcs").quantity_kind(), MeasurementQuantityKind::Count);

    assert_ne!(normalized("1 Nl/h"), normalized("1 l/h"));
    assert_ne!(normalized("60 ct/min"), normalized("1 Hz"));
    assert_ne!(normalized("1 fr/s"), normalized("1 Hz"));
    assert_ne!(normalized("1 GFLOPS"), normalized("1 GHz"));
}


#[test]
fn named_and_compound_measurements_share_specific_quantity_kinds() {
    let quantity_kind = |text: &str| {
        first_measurement(text)
            .measurement_fact()
            .and_then(|fact| fact.values().first().map(MeasurementValueFact::quantity_kind))
            .unwrap_or_else(|| panic!("quantity kind for {text:?}"))
    };

    for (named, compound, expected) in [
        ("1 mph", "1m/s", MeasurementQuantityKind::Speed),
        ("1 ha", "1m²", MeasurementQuantityKind::Area),
        ("1 mmHg", "1 N/m²", MeasurementQuantityKind::Pressure),
        ("1C", "1 A·s", MeasurementQuantityKind::ElectricCharge),
        ("1F", "1 A·s/V", MeasurementQuantityKind::Capacitance),
        ("1H", "1 V·s/A", MeasurementQuantityKind::Inductance),
        ("1Wb", "1 V·s", MeasurementQuantityKind::MagneticFlux),
        ("1T", "1 Wb/m²", MeasurementQuantityKind::MagneticFluxDensity),
        ("1 kWh", "1 W·h", MeasurementQuantityKind::Energy),
        ("1 mAh", "1 mA·h", MeasurementQuantityKind::ElectricCharge),
        ("1 kbps", "1 kbit/s", MeasurementQuantityKind::DataRate),
        ("1°/s", "1°/s", MeasurementQuantityKind::AngularVelocity),
        ("1 MWe", "1 MW", MeasurementQuantityKind::ActivePower),
        ("1 mGy/h", "1 Gy/h", MeasurementQuantityKind::AbsorbedDoseRate),
        ("1 µSv/h", "1 Sv/h", MeasurementQuantityKind::EquivalentDoseRate),
        ("1 mR/h", "1 R/h", MeasurementQuantityKind::RadiationExposureRate),
        ("1 Nl/h", "1 Nl/s", MeasurementQuantityKind::NormalizedVolumeRate),
        ("1 f/s", "1 fr/s", MeasurementQuantityKind::FrameRate),
        ("1 GFLOPS", "1 TFLOPS", MeasurementQuantityKind::ComputationRate),
    ] {
        assert_eq!(quantity_kind(named), expected, "named unit {named:?}");
        assert_eq!(quantity_kind(compound), expected, "compound unit {compound:?}");
    }
}


#[test]
fn measurement_names_and_unicode_aliases_are_locale_aware() {
    for (text, language, canonical) in [
        ("mil", ParseLanguage::Swedish, "mil"),
        ("Meile", ParseLanguage::German, "mi"),
        ("hektar", ParseLanguage::Swedish, "ha"),
        ("Hektar", ParseLanguage::German, "ha"),
        ("Tonnen", ParseLanguage::German, "t"),
        ("kalorier", ParseLanguage::Swedish, "cal"),
    ] {
        assert_eq!(canonical_measurement_unit(text, language).as_deref(), Some(canonical), "text {text:?}");
    }

    for (text, canonical) in [("32℉", "°F"), ("273K", "K")] {
        let measurement = first_measurement(text);
        assert_eq!(measurement.measurement_fact().expect("measurement fact").unit(), canonical);
    }
    assert!(parse_nodes(&crate::tokenized_text_tree("Figure 5 C"), ParseLanguage::English).iter().all(|node| node.kind() != NodeKind::Measurement));
}


#[test]
fn regional_locales_ground_us_and_imperial_capacity_units() {
    let parsed = |text: &str, language_tag: &str| {
        let tree = crate::tokenized_text_tree(text);
        parse_output(&tree, ParseLocale::from_bcp47(language_tag), ParseGrounding::new())
            .nodes
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| (fact.unit().to_string(), fact.values().to_vec())))
    };

    for (text, uk, us) in [
        ("1 fl oz", "fl oz (imp)", "fl oz (US)"),
        ("1 qt", "qt (imp)", "qt (US)"),
        ("1 bu", "bu (imp)", "bu (US)"),
        ("1 gal", "gal (imp)", "gal (US)"),
    ] {
        assert_eq!(parsed(text, "en-GB").map(|fact| fact.0), Some(uk.to_string()), "UK text {text:?}");
        assert_eq!(parsed(text, "en-US").map(|fact| fact.0), Some(us.to_string()), "US text {text:?}");
        assert_eq!(parsed(text, "en"), None, "regionless text {text:?}");
        assert_eq!(parsed(text, "sv-SE"), None, "Swedish text {text:?}");
        assert_eq!(parsed(text, "de-DE"), None, "German text {text:?}");
    }

    assert_eq!(parsed("1 pt", "en-GB").map(|fact| fact.0), Some("pt (imp)".to_string()));
    assert_eq!(parsed("1 pt", "en-US").map(|fact| fact.0), Some("pt (US)".to_string()));
    assert_eq!(parsed("1 pt", "en"), None);
    assert_eq!(parsed("1 pt", "sv-SE").map(|fact| fact.0), Some("pt".to_string()));
    assert_eq!(parsed("1 pt", "de-DE").map(|fact| fact.0), Some("pt".to_string()));
    assert_eq!(parsed("12pt", "en-GB").map(|fact| fact.0), Some("pt".to_string()));
    assert_eq!(parsed("12pt", "en-US").map(|fact| fact.0), Some("pt".to_string()));
    assert_eq!(parsed("1gal", "en-GB").map(|fact| fact.0), Some("gal (imp)".to_string()));
    assert_eq!(parsed("1gal", "en-US").map(|fact| fact.0), Some("gal (US)".to_string()));
    assert_eq!(parsed("1 imperial gallon", "en").map(|fact| fact.0), Some("gal (imp)".to_string()));
    assert_eq!(parsed("1 US liquid gallon", "en").map(|fact| fact.0), Some("gal (US)".to_string()));
    assert_eq!(parsed("1 gal", "en-GB").map(|fact| fact.1), parsed("4.54609L", "en-GB").map(|fact| fact.1));
    assert_eq!(parsed("1 gal", "en-US").map(|fact| fact.1), parsed("3.785411784L", "en-US").map(|fact| fact.1));
    assert_eq!(parsed("1 pt", "en-GB").map(|fact| fact.1), parsed("568.26125ml", "en-GB").map(|fact| fact.1));
    assert_eq!(parsed("1 pt", "en-US").map(|fact| fact.1), parsed("473.176473ml", "en-US").map(|fact| fact.1));
    assert_ne!(parsed("1 gal", "en-GB").map(|fact| fact.1), parsed("1 gal", "en-US").map(|fact| fact.1));
}


#[test]
fn localized_unit_words_are_detected_after_numbers_without_source_grounding() {
    let parsed_unit = |text, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()))
    };

    for (text, language, canonical) in [
        ("5 seconds", ParseLanguage::English, "s"),
        ("5 hours", ParseLanguage::English, "h"),
        ("5 pounds", ParseLanguage::English, "lb"),
        ("5 micrograms", ParseLanguage::English, "µg"),
        ("5 picometers", ParseLanguage::English, "pm"),
        ("5 decameters", ParseLanguage::English, "dam"),
        ("5 hectometers", ParseLanguage::English, "hm"),
        ("5 horsepower", ParseLanguage::English, "HP"),
        ("5 sekunder", ParseLanguage::Swedish, "s"),
        ("5 timmar", ParseLanguage::Swedish, "h"),
        ("5 pund", ParseLanguage::Swedish, "lb"),
        ("5 fötter", ParseLanguage::Swedish, "ft"),
        ("5 mikrogram", ParseLanguage::Swedish, "µg"),
        ("5 hästkrafter", ParseLanguage::Swedish, "PS"),
        ("5 pikometer", ParseLanguage::Swedish, "pm"),
        ("5 centiliter", ParseLanguage::Swedish, "cl"),
        ("5 deciliter", ParseLanguage::Swedish, "dl"),
        ("5 dekameter", ParseLanguage::Swedish, "dam"),
        ("5 hektometer", ParseLanguage::Swedish, "hm"),
        ("5 Sekunden", ParseLanguage::German, "s"),
        ("5 Stunden", ParseLanguage::German, "h"),
        ("5 Pfund", ParseLanguage::German, "lb"),
        ("5 Füßen", ParseLanguage::German, "ft"),
        ("5 Unzen", ParseLanguage::German, "oz"),
        ("5 Mikrogramm", ParseLanguage::German, "µg"),
        ("5 Pferdestärken", ParseLanguage::German, "PS"),
        ("5 Pikometer", ParseLanguage::German, "pm"),
        ("5 Zentimeter", ParseLanguage::German, "cm"),
        ("5 Deziliter", ParseLanguage::German, "dl"),
        ("5 Dekameter", ParseLanguage::German, "dam"),
        ("5 Hektometer", ParseLanguage::German, "hm"),
        ("5 Kilometern", ParseLanguage::German, "km"),
        ("5 Millilitern", ParseLanguage::German, "ml"),
    ] {
        assert_eq!(parsed_unit(text, language).as_deref(), Some(canonical), "text {text:?}");
    }

    for (text, language) in [
        ("5 micrograms", ParseLanguage::Swedish),
        ("5 micrograms", ParseLanguage::German),
        ("5 picometers", ParseLanguage::Swedish),
        ("5 picometers", ParseLanguage::German),
        ("5 mikrogram", ParseLanguage::English),
        ("5 zentimeter", ParseLanguage::English),
        ("5 centimeter", ParseLanguage::German),
        ("5 grams", ParseLanguage::Swedish),
        ("5 watts", ParseLanguage::German),
        ("5 yards", ParseLanguage::German),
        ("5 yards", ParseLanguage::Swedish),
    ] {
        assert_eq!(parsed_unit(text, language), None, "foreign word form {text:?}");
    }
}

#[test]
fn expanded_unit_forms_require_matching_source_grounding() {
    for (text, language, known_unit) in [
        ("5 beats per minute", ParseLanguage::English, "bpm"),
        ("5 heartbeats per minute", ParseLanguage::English, "bpm"),
        ("5 cycles per second", ParseLanguage::English, "Hz"),
        ("5 frames per second", ParseLanguage::English, "fr/s"),
        ("5 parts per million", ParseLanguage::English, "ppm"),
        ("5 watt hours", ParseLanguage::English, "Wh"),
        ("5 ampere hours", ParseLanguage::English, "Ah"),
        ("5 metres per second", ParseLanguage::English, "m/s"),
        ("5 cubic metres per hour", ParseLanguage::English, "m³/h"),
        ("5 kilograms per cubic metre", ParseLanguage::English, "kg/m³"),
        ("5 kilowatt hours", ParseLanguage::English, "kWh"),
        ("5 slag per minut", ParseLanguage::Swedish, "bpm"),
        ("5 hjärtslag per minut", ParseLanguage::Swedish, "bpm"),
        ("5 cykler per sekund", ParseLanguage::Swedish, "Hz"),
        ("5 varv per minut", ParseLanguage::Swedish, "rpm"),
        ("5 delar per miljon", ParseLanguage::Swedish, "ppm"),
        ("5 wattimmar", ParseLanguage::Swedish, "Wh"),
        ("5 amperetimmar", ParseLanguage::Swedish, "Ah"),
        ("5 kilometer per timme", ParseLanguage::Swedish, "km/h"),
        ("5 watt per kvadratmeter", ParseLanguage::Swedish, "W/m²"),
        ("5 kilowattimmar", ParseLanguage::Swedish, "kWh"),
        ("5 Schläge pro Minute", ParseLanguage::German, "bpm"),
        ("5 Herzschläge pro Minute", ParseLanguage::German, "bpm"),
        ("5 Zyklen pro Sekunde", ParseLanguage::German, "Hz"),
        ("5 Bilder pro Sekunde", ParseLanguage::German, "fr/s"),
        ("5 Teile pro Million", ParseLanguage::German, "ppm"),
        ("5 Wattstunden", ParseLanguage::German, "Wh"),
        ("5 Amperestunden", ParseLanguage::German, "Ah"),
        ("5 Liter pro Minute", ParseLanguage::German, "l/min"),
        ("5 Newtonmeter", ParseLanguage::German, "N·m"),
        ("5 Kilowattstunden", ParseLanguage::German, "kWh"),
    ] {
        let standalone = parse_nodes(&crate::tokenized_text_tree(text), language);
        let source_grounded = parse_nodes_with_known_units(&crate::tokenized_text_tree(text), language, &[known_unit]);
        let different_unit = parse_nodes_with_known_units(&crate::tokenized_text_tree(text), language, &["rpm"]);

        assert_ne!(standalone.iter().find_map(|node| node.measurement_fact().map(|fact| fact.unit())), Some(known_unit), "{text}");
        assert_eq!(source_grounded.iter().find_map(|node| node.measurement_fact().map(|fact| fact.unit())), Some(known_unit), "{text}");
        if known_unit != "rpm" {
            assert_ne!(different_unit.iter().find_map(|node| node.measurement_fact().map(|fact| fact.unit())), Some(known_unit), "{text}");
        }
    }
}


#[test]
fn localized_multiword_unit_names_use_longest_whitespace_normalized_match() {
    let parsed_unit = |text, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()))
    };

    for (text, language, canonical) in [
        ("20 degrees Celsius", ParseLanguage::English, "°C"),
        ("20 per cent", ParseLanguage::English, "%"),
        ("5 metric tons", ParseLanguage::English, "t"),
        ("20 grad Celsius", ParseLanguage::Swedish, "°C"),
        ("20 Grad Celsius", ParseLanguage::German, "°C"),
        ("20 degrees\u{00A0}Celsius", ParseLanguage::English, "°C"),
    ] {
        assert_eq!(parsed_unit(text, language).as_deref(), Some(canonical), "text {text:?}");
    }
}


#[test]
fn locale_specific_measurement_aliases_require_their_own_language_and_case() {
    let parsed_unit = |text, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()))
    };

    for (text, language, canonical) in [
        ("5 in", ParseLanguage::English, "in"),
        ("5 inch", ParseLanguage::English, "in"),
        ("5 hr", ParseLanguage::English, "h"),
        ("5 amp", ParseLanguage::English, "A"),
        ("5 hp", ParseLanguage::English, "HP"),
        ("5 bn", ParseLanguage::English, "bn"),
        ("5 mil", ParseLanguage::Swedish, "mil"),
        ("5 hk", ParseLanguage::Swedish, "PS"),
        ("5 sek", ParseLanguage::Swedish, "s"),
        ("5 tim", ParseLanguage::Swedish, "h"),
        ("5 mn", ParseLanguage::Swedish, "mn"),
        ("5 mdr", ParseLanguage::Swedish, "mdr"),
        ("5 ml/sek", ParseLanguage::Swedish, "ml/s"),
        ("5 km/tim", ParseLanguage::Swedish, "km/h"),
        ("5 qm", ParseLanguage::German, "m²"),
        ("5 mg/qm", ParseLanguage::German, "mg/m²"),
        ("5 U/min", ParseLanguage::German, "rpm"),
        ("5 h/Tag", ParseLanguage::German, "h/d"),
        ("5 Mbit/Sek", ParseLanguage::German, "Mbit/s"),
        ("5 Mio", ParseLanguage::German, "Mio"),
        ("5 Mrd", ParseLanguage::German, "Mrd"),
        ("5 Sek", ParseLanguage::German, "s"),
        ("5 sek", ParseLanguage::German, "s"),
        ("5 Std", ParseLanguage::German, "h"),
        ("5 std", ParseLanguage::German, "h"),
        ("5 Mbit/sek", ParseLanguage::German, "Mbit/s"),
        ("5 km/Std", ParseLanguage::German, "km/h"),
    ] {
        assert_eq!(parsed_unit(text, language).as_deref(), Some(canonical), "text {text:?}");
    }

    for (text, allowed_language, other_languages) in [
        ("5 mil", ParseLanguage::Swedish, [ParseLanguage::English, ParseLanguage::German]),
        ("5 hk", ParseLanguage::Swedish, [ParseLanguage::English, ParseLanguage::German]),
        ("5 mn", ParseLanguage::Swedish, [ParseLanguage::English, ParseLanguage::German]),
        ("5 mdr", ParseLanguage::Swedish, [ParseLanguage::English, ParseLanguage::German]),
        ("5 in", ParseLanguage::English, [ParseLanguage::Swedish, ParseLanguage::German]),
        ("5 hr", ParseLanguage::English, [ParseLanguage::Swedish, ParseLanguage::German]),
        ("5 amp", ParseLanguage::English, [ParseLanguage::Swedish, ParseLanguage::German]),
        ("5 bn", ParseLanguage::English, [ParseLanguage::Swedish, ParseLanguage::German]),
        ("5 qm", ParseLanguage::German, [ParseLanguage::English, ParseLanguage::Swedish]),
        ("5 mg/qm", ParseLanguage::German, [ParseLanguage::English, ParseLanguage::Swedish]),
        ("5 U/min", ParseLanguage::German, [ParseLanguage::English, ParseLanguage::Swedish]),
        ("5 Mio", ParseLanguage::German, [ParseLanguage::English, ParseLanguage::Swedish]),
        ("5 Mrd", ParseLanguage::German, [ParseLanguage::English, ParseLanguage::Swedish]),
    ] {
        let allowed = parsed_unit(text, allowed_language).expect("allowed locale alias");
        other_languages.into_iter().for_each(|language| assert_ne!(parsed_unit(text, language).as_deref(), Some(allowed.as_str()), "foreign alias {text:?}"));
    }

    for (text, language) in [
        ("5 HP", ParseLanguage::English),
        ("5 HP", ParseLanguage::Swedish),
        ("5 HP", ParseLanguage::German),
        ("5 PS", ParseLanguage::English),
        ("5 PS", ParseLanguage::Swedish),
        ("5 PS", ParseLanguage::German),
        ("5 sec", ParseLanguage::English),
        ("5 sec", ParseLanguage::German),
    ] {
        assert!(parsed_unit(text, language).is_some(), "technical or legacy symbol {text:?}");
    }
    assert_eq!(parsed_unit("5 sec", ParseLanguage::Swedish), None);

    for (text, language) in [
        ("5 HK", ParseLanguage::Swedish),
        ("5 Tim", ParseLanguage::Swedish),
        ("5 SEK", ParseLanguage::Swedish),
        ("5 Hp", ParseLanguage::English),
        ("5 STD", ParseLanguage::German),
    ] {
        assert_eq!(parsed_unit(text, language), None, "unsupported case variant {text:?}");
    }

    for (text, language) in [
        ("5 mg/qm", ParseLanguage::English),
        ("5 mg/qm", ParseLanguage::Swedish),
        ("5 Mbit/Sek", ParseLanguage::English),
        ("5 Mbit/Sek", ParseLanguage::Swedish),
        ("5 h/Tag", ParseLanguage::English),
        ("5 h/Tag", ParseLanguage::Swedish),
        ("5 ml/sek", ParseLanguage::English),
        ("5 km/tim", ParseLanguage::English),
        ("5 Mbit/sek", ParseLanguage::English),
        ("5 km/Std", ParseLanguage::English),
        ("5 m/sec", ParseLanguage::Swedish),
        ("5 KB/sec", ParseLanguage::Swedish),
        ("5 °C/sec", ParseLanguage::Swedish),
        ("5 mm/hr", ParseLanguage::Swedish),
        ("5 mm/hr", ParseLanguage::German),
    ] {
        assert_eq!(parsed_unit(text, language), None, "foreign compound alias {text:?} must not be truncated");
    }
}


#[test]
fn numeric_magnitude_suffixes_are_classified_outside_physical_unit_resolution() {
    for (text, language) in [
        ("M", ParseLanguage::English),
        ("M", ParseLanguage::Swedish),
        ("M", ParseLanguage::German),
        ("bn", ParseLanguage::English),
        ("mn", ParseLanguage::Swedish),
        ("mdr", ParseLanguage::Swedish),
        ("Mio", ParseLanguage::German),
        ("Mrd", ParseLanguage::German),
    ] {
        assert_eq!(canonical_measurement_unit(text, language), None, "magnitude {text:?} must not be a physical unit");
        assert!(resolve_measurement_unit(text, language).is_none(), "magnitude {text:?} must not have physical semantics");
        let parsed = parse_nodes(&crate::tokenized_text_tree(format!("5 {text}")), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()));
        assert_eq!(parsed.as_deref(), Some(text), "numeric QA must retain magnitude {text:?}");
    }
}


#[test]
fn english_inch_abbreviation_is_language_scoped() {
    assert_eq!(canonical_measurement_unit("in", ParseLanguage::English).as_deref(), Some("in"));
    assert_eq!(canonical_measurement_unit("inch", ParseLanguage::English).as_deref(), Some("in"));
    assert!(resolve_measurement_unit("in/s", ParseLanguage::English).is_some());

    for language in [ParseLanguage::Swedish, ParseLanguage::German] {
        assert_eq!(canonical_measurement_unit("in", language), None);
        assert_eq!(canonical_measurement_unit("inch", language), None);
        assert!(resolve_measurement_unit("in/s", language).is_none());
    }

    let parsed_unit = |text, language| {
        parse_nodes(&crate::tokenized_text_tree(text), language)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.unit().to_string()))
    };
    assert_eq!(parsed_unit("5 in.", ParseLanguage::English).as_deref(), Some("in"));
    assert_eq!(parsed_unit("5 in.", ParseLanguage::Swedish), None);
    assert_eq!(parsed_unit("5 in.", ParseLanguage::German), None);
}


#[test]
fn measurement_typography_is_owned_by_parser_semantics() {
    let typography = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| fact.typography()))
            .expect("measurement typography")
    };

    let compact = typography("5mg");
    assert_eq!(compact.whitespace_fact().observed(), WhitespaceTypographyKind::Absent);
    assert_eq!(compact.whitespace_fact().preferred(), Some(WhitespaceTypographyKind::NonBreaking));

    let temperature = typography("20 ºC");
    assert_eq!(temperature.whitespace_fact().observed(), WhitespaceTypographyKind::Regular);
    assert_eq!(temperature.degree_style(), MeasurementDegreeStyle::Variant);

    let angle = typography("90°");
    assert_eq!(angle.whitespace_fact().preferred(), Some(WhitespaceTypographyKind::Absent));
    assert_eq!(angle.degree_style(), MeasurementDegreeStyle::Canonical);
}


#[test]
fn canonicalizes_general_unit_aliases() {
    for (text, canonical) in [
        ("5 Gb/s", "Gbit/s"),
        ("5 Gbyte", "GB"),
        ("5 Gflops", "GFLOPS"),
        ("5 KHz", "kHz"),
        ("5 KWh/m", "kWh/m"),
        ("5 Kbit/s", "kbit/s"),
        ("5 Pbyte", "PB"),
        ("5 Pflops", "PFLOPS"),
        ("5 Tb/s", "Tbit/s"),
        ("5 Tbyte", "TB"),
        ("5 Tflops", "TFLOPS"),
        ("5 gigaflops", "GFLOPS"),
        ("5 kb/s", "kbit/s"),
        ("5 kbps", "kbit/s"),
        ("5 mAs", "mA·s"),
        ("5 oC", "°C"),
        ("5 oF", "°F"),
        ("5 petaflops", "PFLOPS"),
        ("5 terraflops", "TFLOPS"),
        ("5 ug", "µg"),
        ("5 ul/min", "µl/min"),
        ("5 um", "µm"),
        ("5 us", "µs"),
        ("5 °K", "K"),
        ("5 °K/h", "K/h"),
        ("5 °С", "°C"),
        ("5 ºC", "°C"),
        ("5 ºF", "°F"),
        ("5 ˚", "°"),
        ("5 ˚C", "°C"),
        ("5 ˚F", "°F"),
        ("5 ℃", "°C"),
        ("5 Ohm", "Ω"),
        ("5 mm2", "mm²"),
    ] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let measurement = nodes.iter().find(|node| node.kind() == NodeKind::Measurement).expect("measurement");
        assert_eq!(measurement.measurement_fact().expect("measurement fact").unit(), canonical, "text {text}");
    }
}


#[test]
fn preserves_term_units_from_tokenizer() {
    let units = vec![
        u(UnitKind::Word, "See", 3),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Term, "Yellowstone", 11),
        u(UnitKind::Whitespace, " ", 1),
        u(UnitKind::Word, "here", 4),
    ];
    let nodes = parse_nodes(&stream(units, "See Yellowstone here"), ParseLanguage::Neutral);
    assert!(nodes.iter().any(|node| node.kind() == NodeKind::Term));
}


#[test]
fn spelled_weight_unit_is_detected_from_adjacent_number() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("8 pounds"), ParseLanguage::English);
    let measurement = nodes.iter().find(|node| node.kind() == NodeKind::Measurement).expect("measurement");
    assert_eq!(measurement.measurement_fact().expect("measurement fact").unit(), "lb");

    let text = "8 pounds";
    let nodes = parse_nodes_with_known_units(&crate::tokenized_text_tree(text), ParseLanguage::English, &["lb"]);
    let measurement = nodes.iter().find(|node| node.kind() == NodeKind::Measurement).expect("known unit measurement");
    assert_eq!(&text[measurement.span()], text);
}


#[test]
fn parser_detects_measurement_ranges_and_dimensions() {
    for text in ["10-20 mm", "10–20 mm", "600x400 px"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        let measurement = nodes.iter().find(|node| node.kind() == NodeKind::Measurement).unwrap_or_else(|| panic!("measurement for {text:?}"));
        assert_eq!(&text[measurement.span()], text);
    }

    let nodes = parse_nodes(&crate::tokenized_text_tree("600×400"), ParseLanguage::Neutral);
    let dimension = nodes.iter().find(|node| node.kind() == NodeKind::Number).expect("dimension number");
    assert_eq!(&"600×400"[dimension.span()], "600×400");
}

#[test]
fn measurement_and_number_ranges_share_the_locale_separator_catalog() {
    for separator in ParseLanguage::Neutral.profile().range_separators.symbolic {
        let measurement_text = format!("10{separator}20 mm");
        let measurement_nodes = parse_nodes(&crate::tokenized_text_tree(&measurement_text), ParseLanguage::Neutral);
        let measurement = measurement_nodes.iter().find(|node| node.kind() == NodeKind::Measurement).unwrap_or_else(|| panic!("measurement for {measurement_text:?}"));
        assert_eq!(measurement.span(), 0..measurement_text.len());
        assert_eq!(measurement.measurement_fact().expect("measurement fact").number_values().len(), 2);

        let range_text = format!("10{separator}20");
        let range_nodes = parse_nodes(&crate::tokenized_text_tree(&range_text), ParseLanguage::Neutral);
        assert!(range_nodes.iter().any(|node| node.kind() == NodeKind::NumberRange && node.span() == (0..range_text.len())), "number range for {range_text:?}");
    }
}


#[test]
fn parser_does_not_treat_words_as_measurement_range_separators() {
    for text in ["10 to 20 mm", "10 bis 20 mm", "10 till 20 mm"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Neutral);
        assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Measurement && node.span() == (0..text.len())), "text {text}");
    }
}


#[test]
fn parser_does_not_merge_spaced_dimension_without_unit() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("3 x 4 = 12"), ParseLanguage::English);
    assert!(!nodes.iter().any(|node| node.kind() == NodeKind::Number && node.span() == (0..5)));
}


#[test]
fn measurement_ranges_carry_each_numeric_value_fact() {
    let nodes = parse_nodes(&crate::tokenized_text_tree("10-20 mm"), ParseLanguage::Neutral);
    let measurement = nodes.iter().find(|node| node.kind() == NodeKind::Measurement).expect("measurement");
    let values = measurement.measurement_fact().expect("measurement fact").values().len();
    assert_eq!(values, 2);
}

#[test]
fn repeated_unit_measurement_ranges_are_one_measurement() {
    for text in ["15 %–95 %", "15% – 95%", "15 % - 95 %"] {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::Swedish);
        let measurements = nodes.iter().filter(|node| node.kind() == NodeKind::Measurement).collect::<Vec<_>>();

        assert_eq!(measurements.len(), 1, "{text}");
        assert_eq!(measurements[0].measurement_fact().expect("measurement fact").values().len(), 2, "{text}");
    }
}


#[test]
fn swedish_leading_number_with_suffix_does_not_get_stuck() {
    let parse_language = ParseLanguage::Swedish;
    let cases = [
        ("1", NodeKind::Number),
        ("1.", NodeKind::Number),
        ("1%", NodeKind::Measurement),
        ("1+a", NodeKind::Number),
        ("1:a", NodeKind::Ordinal),
    ];
    for (text, expected) in cases {
        let nodes = parse_nodes(&crate::tokenized_text_tree(text), parse_language);
        let has_kind = nodes.iter().any(|node| node.kind() == expected);
        assert!(has_kind, "text {text} expected {expected:?}");
    }
}

#[test]
fn energy_units_without_safe_fixed_point_conversions_retain_identity() {
    for (text, canonical) in [("7 Btu", "Btu"), ("7BTU", "Btu"), ("7 therm", "therm"), ("7 British thermal units", "Btu")] {
        let fact = parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().map(|fact| (fact.unit().to_string(), fact.values().to_vec())))
            .unwrap_or_else(|| panic!("measurement for {text:?}"));
        assert_eq!(fact.0, canonical, "text {text:?}");
        assert!(fact.1.is_empty(), "opaque unit {text:?} must not claim normalized equivalence");
    }
    assert!(resolve_measurement_unit("Btu", ParseLanguage::English).is_none());
    assert_eq!(canonical_measurement_unit("BTU", ParseLanguage::English).as_deref(), Some("Btu"));
}


#[test]
fn hectare_megapixel_and_great_gross_have_semantic_conversions() {
    let normalized = |text: &str| {
        parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English)
            .into_iter()
            .find_map(|node| node.measurement_fact().and_then(|fact| fact.values().first().copied()))
            .unwrap_or_else(|| panic!("normalized measurement for {text:?}"))
    };

    assert_eq!(normalized("1 HA"), normalized("1 ha"));
    assert_eq!(normalized("1 MP"), normalized("1000000 px"));
    assert_eq!(normalized("1 ggr"), normalized("1728 pcs"));
    assert_eq!(normalized("1 MP").quantity_kind(), MeasurementQuantityKind::PixelCount);
    assert_eq!(normalized("1 MP/s").quantity_kind(), MeasurementQuantityKind::PixelRate);
    assert_ne!(normalized("1 px"), normalized("1 pcs"));
}
