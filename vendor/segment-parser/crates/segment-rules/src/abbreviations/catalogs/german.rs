//! Existing German abbreviation catalog.
use super::super::{AbbreviationCase, AbbreviationEvidence, AbbreviationPolicy, AbbreviationRule};
use super::common::{ENGLISH_GERMAN_ABBREVIATIONS, ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS, GERMAN_SWEDISH_ABBREVIATIONS};

pub(crate) static GERMAN_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&[
        "z.b.", "d.h.", "bzw.", "usw.", "ggf.", "vgl.", "nachm.",
        "s.o.", "s.u.", "evtl.", "hrsg.", "aufl.", "bzgl.", "bd.", "forts.", "abb.", "abt.", "anh.", "abschn.", "rev.",
        "spez.", "str.", "abzgl.", "allg.", "amtl.", "angew.", "anschl.", "angest.", "anerk.", "ausg.", "ausgeschl.", "ausschl.", "benutzerdef.",
        "berufl.", "biogr.", "bspw.", "chem.", "chir.", "christl.", "durchschn.", "ebd.", "ehem.", "eigtl.", "einschl.", "eingetr.", "einh.",
        "entspr.", "europ.", "fortl.", "fotogr.", "freiw.", "gebr.", "gegr.", "geschl.", "ggfs.", "hergest.", "insb.", "insbes.", "insges.", "intell.", "jhrl.",
        "kaufm.", "komb.", "konstr.", "künstl.", "landw.", "minderj.", "mögl.", "mwst.", "nachtr.", "notw.", "oberfl.", "prakt.", "regelm.", "rechtsw.", "restl.",
        "richtl.", "schriftl.", "seitw.", "staatl.", "stat.", "stellv.", "teilw.", "telef.", "techn.", "tägl.", "unvollst.", "urspr.", "vollst.", "wahrsch.", "wesentl.",
        "zeitl.", "zeitw.", "ztg.", "öffentl.", "öfftl.", "ökon.", "örtl.", "überschr.",
    ], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&[
        "vorm.", "sog.", "vers.", "akt.", "sogen.", "werkt.", "div.", "geh.", "log.", "reg.",
    ], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Continuation),
    AbbreviationRule::new(&[
        "So.",
    ], AbbreviationCase::ExactCase, AbbreviationEvidence::Continuation),
    AbbreviationRule::new(&[
        "Jh.",
    ], AbbreviationCase::ExactCase, AbbreviationEvidence::PrecedingReference),
    AbbreviationRule::new(&[
        "Abf.", "Abg.", "Abk.", "Abs.", "Akad.", "Angeh.", "Anl.", "Anm.", "Arb.", "Assist.", "Ausbild.", "Ausstattg.", "Bauj.", "Beisp.", "Bem.", "Betr.",
        "Bevollm.", "Bez.", "Bhf.", "Bsp.", "Buchf.", "Darst.", "Dat.", "Dipl.", "Diss.", "Doz.", "Drs.", "Eidg.", "Einf.", "Einschr.", "Elektr.", "Fabr.",
        "Fernschr.", "Fahrz.", "Fam.", "Frl.", "Geb.", "Gef.", "Gem.", "Gesch.", "Gez.", "Hbf.", "Inf.", "Inform.", "Inh.", "Ing.", "Herst.", "Jur.",
        "Kapt.", "Kennz.", "Kfm.", "Landkr.", "Lfd.", "Lief.", "Ltg.", "Mrd.", "Nachf.", "Notf.", "Offiz.", "Parl.", "Pers.", "Pkt.", "Priv.",
        "Proz.", "Psych.", "Rep.", "Sachb.", "Samml.", "Sem.", "Seriennr.", "Soz.", "Temp.", "Teilenr.", "Umf.", "Vors.", "Whg.", "Zentr.",
        "Ziff.", "Zlg.", "Ztschr.",
    ], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
    // Trados' German resources are substantially broader than its English
    // resources. These groups retain only entries that are not ordinary
    // lowercase words, one-letter initials, hyphen forms, or dotted
    // initialisms already recognized by the parser.
    // Lexical collisions deliberately excluded here are documented in
    // `research/TRADOS_ABBREVIATION_PARITY.md`.
    AbbreviationRule::new(&[
        "abl.", "abr.", "akk.", "alk.", "allj.", "app.", "arab.", "asym.", "atm.", "ausgen.", "austral.", "ausw.", "auth.", "autom.", "auton.", "bde.",
        "bds.", "beamt.", "bef.", "beg.", "begl.", "beif.", "bek.", "belg.", "ber.", "bes.", "besch.", "beurl.", "bsd.", "bwgl.", "cand.", "dan.",
        "demn.", "ders.", "desgl.", "dgl.", "dienstl.", "diesj.", "dkr.", "dstl.", "dtzd.", "durchw.", "dyn.", "ehel.", "ehm.", "ehrw.", "eidgen.", "einwdfr.",
        "einz.", "empf.", "endg.", "entf.", "entggs.", "enth.", "entw.", "erb.", "erg.", "erh.", "erl.", "erm.", "ers.", "erstkl.", "etw.", "eur.",
        "ev.", "evangel.", "evt.", "feingeh.", "finn.", "folg.", "freundl.", "frz.", "fut.", "gastr.", "gek.", "geogr.", "gepr.",
        "gesp.", "gest.", "getr.", "gew.", "ggs.", "ggw.", "ggz.", "ggü.", "gkl.", "glt.", "gltg.", "gms.", "gründl.", "gyn.", "gzj.", "habil.",
        "haftb.", "haftpfl.", "halbj.", "halbjhl.", "haltb.", "handwerkl.", "helv.", "herg.", "hins.", "hist.", "hochd.", "holl.", "hpts.", "hydr.", "hyp.", "hypoth.",
        "hzb.", "hzg.", "hzgl.", "inbegr.", "incl.", "indir.", "innerl.", "inoff.", "int.", "inv.", "inwf.", "inww.", "ital.", "jr.", "kal.", "kameradschaftl.",
        "kan.", "kard.", "kath.", "kgl.", "kl.", "komf.", "komp.", "kond.", "konf.", "konj.", "kontr.", "konz.", "kpl.", "krit.", "lab.", "lat.",
        "lbd.", "ldg.", "led.", "lekt.", "ler.", "lfg.", "lfgn.", "lfzt.", "lgfr.", "lit.", "lsg.", "luftd.", "luth.", "masch.", "maschr.", "math.",
        "mdl.", "med.", "mex.", "mhd.", "mil.", "milit.", "mill.", "min.", "mind.", "mod.", "moh.", "mot.", "mst.", "mtl.", "multilat.",
        "männl.", "möbl.", "nat.", "neg.", "neutr.", "neuw.", "nom.", "norw.", "näml.", "obh.", "oblig.", "offz.", "ord.", "orth.", "pfd.",
        "phil.", "photogr.", "plötzl.", "poln.", "portug.", "postw.", "prot.", "präs.", "päd.", "rechtl.", "rel.", "res.", "resp.",
        "russ.", "röm.", "rückf.", "rücks.", "rückw.", "sakr.", "schw.", "schwed.", "scil.", "sec.", "sign.", "skand.", "sof.", "sowj.",
        "spp.", "std.", "stdl.", "synth.", "tabl.", "telegr.", "tgl.", "tlg.", "tlw.", "tödl.", "türk.",
        "unbegr.", "unbek.", "unbez.", "unehel.", "unentsch.", "unerw.", "unfrw.", "ung.", "ungebr.", "ungeg.", "ungek.", "unreg.", "unregelm.", "unt.", "unterh.", "unterz.",
        "untrz.", "unverb.", "unverh.", "url.", "usf.", "var.", "vbdl.", "vdg.", "ver.", "verantw.", "verbdg.", "verf.", "vergl.", "verh.", "verk.", "verl.",
        "verp.", "verpfl.", "versch.", "vertr.", "verw.", "verz.", "veröff.", "vet.", "vfg.", "vorbeh.", "vorbest.", "vorl.", "wbl.", "wdh.", "wirtsch.", "wiss.",
        "zahlr.", "zeitgen.", "zgl.", "ziv.", "ztr.", "zub.", "zuf.", "zugel.", "zugl.", "zul.", "zus.", "zuzgl.", "zzgl.", "öff.", "übl.",
    ], AbbreviationCase::CaseInsensitiveExceptAllCaps, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&[
        "Afr.", "Ag.", "Anon.", "Apr.", "Art.Nr.", "Ausbildg.", "Ausl.", "Ausn.", "Ausstatt.", "Austr.", "Bayr.", "Bev.", "Bj.", "Brit.", "Bst.", "Ch.",
        "Corp.", "Dachverb.", "Dept.", "Dez.", "Di.", "Distr.", "Do.", "Dolm.", "Dopp.", "Doppelz.", "Dupl.", "El.", "Essl.", "Expl.", "Fa.",
        "Fak.", "Febr.", "Fig.", "Fin.", "Fl.", "Frankf.", "Frankr.", "Friesl.", "Gefl.", "Ges.", "Geschw.", "Gr.", "Grdfl.", "Gymn.", "Hambg.",
        "Hftg.", "Hubr.", "Inc.", "Intl.", "Jap.", "Jun.", "Kat.", "Koeff.", "Kr.", "Krhs.", "Krs.", "Lkr.", "Ltd.",
        "Magn.", "Marktkap.", "Mi.", "Mitbest.", "Mo.", "Mr.", "Mrs.", "Mrz.", "Mär.", "Nbk.", "Nchf.", "Nhd.", "Nmtl.", "Nrn.", "OGw.",
        "Okt.", "Op.", "Pf.", "Pl.", "Prf.", "Prfg.", "Sa.", "Sachnr.", "Sept.", "Ser.Nr.", "Ser.nr.", "SFr.", "Sfr.", "Sr.",
        "Stk.", "Stromkreisnr.", "Stv.", "Teel.", "Th.", "Vol.", "Vorw.", "Wdhlg.", "Wg.", "Wz.", "Württ.", "Würzb.", "z.Bsp.", "z.Hd.", "z.Zt.", "Zust.",
        "Zw.", "ZWSt.", "Zwst.", "Österr.",
    ], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
];
static GERMAN_ABBREVIATION_CATALOGS: &[&[AbbreviationRule]] = &[ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS, ENGLISH_GERMAN_ABBREVIATIONS, GERMAN_SWEDISH_ABBREVIATIONS, GERMAN_ABBREVIATIONS];

pub const GERMAN_POLICY: AbbreviationPolicy = AbbreviationPolicy::new(GERMAN_ABBREVIATION_CATALOGS, true);
