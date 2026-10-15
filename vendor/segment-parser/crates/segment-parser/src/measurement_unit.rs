macro_rules! define_base_units {
    ($($unit:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum BaseUnit { $($unit),+ }

        impl BaseUnit {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$unit),+];
        }
    };
}

define_base_units!(
    Meter, Angstrom, Gram, Second, Liter, Watt, Volt, Ampere, Hertz, Byte, Bit, Mole, Newton, Pascal, StandardAtmosphere, Torr, Joule, Ohm, Siemens, Bar,
    Coulomb, Farad, Henry, Weber, Tesla, Calorie, WattHour, AmpereHour, VoltAmpere, VoltAmpereReactive, ElectricWatt,
    Gray, Sievert, Roentgen, Becquerel, Candela, Lumen, Lux, Radian, Steradian, Decibel, AWeightedDecibel, AlternatingCurrentVolt,
    NormalLitre, PieceCount, GreatGross, Pixel, Megapixel, CountsPerMinute, FramesPerSecond, Flops, LinePairsPerMillimetre,
    Celsius, Fahrenheit, Percent, PartsPerMillion, Minute, Hour, Day, JulianYear, BeatsPerMinute, Foot, Inch, Yard, Mile, NauticalMile, Knot, Pound, Ounce, Grain, Point, Em, Kelvin,
    PoundForcePerSquareInch, ElectronVolt, Dalton, RadiationRem, AstronomicalUnit, CatalyticActivity, Neper, BritishThermalUnit, Therm,
    ShortHundredweight, LongHundredweight,
    ImperialFluidOunce, UsFluidOunce, ImperialPint, UsLiquidPint, ImperialQuart, UsLiquidQuart,
    ImperialBushel, UsBushel, ImperialGallon, UsLiquidGallon,
    Degree, ArcMinute, ArcSecond, ArcThird, DegreesPerSecond,
    Hectare, Acre, Furlong, Tonne, ScandinavianMile, MillimetreOfMercury, InchOfMercury, MillimetreOfWater, CentimetreOfWater, InchOfWater,
    Centipoise, Centistokes, MilesPerHour, RevolutionsPerMinute, MetricHorsepower, MechanicalHorsepower,
    Curie, Jansky, Dyne, Erg, Oersted, Maxwell, Equivalent, Osmole,
);
