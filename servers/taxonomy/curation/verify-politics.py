#!/usr/bin/env python3
"""Requirement-level checks for a frozen politics-review before/after pair."""
import importlib.util
import json
from pathlib import Path
import sqlite3
import csv
import io
import subprocess
import sys
import unittest

FOLDER=Path(sys.argv.pop(1))
LIVE='--live' in sys.argv
if LIVE:sys.argv.remove('--live')
spec=importlib.util.spec_from_file_location('politics',Path(__file__).with_name('improve-politics.py'))
politics=importlib.util.module_from_spec(spec)
spec.loader.exec_module(politics)


class PoliticsReview(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.before=sqlite3.connect(f'file:{FOLDER}/before.sqlite3?mode=ro',uri=True)
        cls.after=sqlite3.connect(f'file:{FOLDER}/after.sqlite3?mode=ro',uri=True)
        cls.manifest=json.loads((FOLDER/'manifest.json').read_text())
        cls.new=cls.manifest['created']

    def parents(self,id):
        return {r[0] for r in self.after.execute('SELECT parent_concept_id FROM concept_parent WHERE concept_id=?',(id,))}

    def label(self,id):
        return self.after.execute('SELECT preferred_label FROM concept WHERE concept_id=?',(id,)).fetchone()[0]

    def test_integrity_cycles_and_reproducible_plan(self):
        politics.validate(self.after)
        copy=sqlite3.connect(':memory:')
        self.before.backup(copy)
        copy.execute('PRAGMA foreign_keys=ON')
        politics.improve(copy)
        self.assertEqual(politics.snapshot(copy),politics.snapshot(self.after))

    def test_no_new_sibling_label_collisions(self):
        sql='SELECT p.parent_concept_id,c.preferred_label FROM concept c JOIN concept_parent p USING(concept_id) GROUP BY p.parent_concept_id,c.preferred_label HAVING count(*)>1'
        self.assertLessEqual(set(self.after.execute(sql)),set(self.before.execute(sql)))

    def test_consolidation_preserves_children_and_shared_routes(self):
        for label in ('Geopolitics','Human & Civil Rights'):
            self.assertEqual(self.after.execute('SELECT count(*) FROM concept WHERE preferred_label=?',(label,)).fetchone()[0],1)
        self.assertEqual(self.parents(5338),{5690,5693})
        self.assertEqual(self.parents(5330),{5690,5692})
        for id in (52586,52587,52588):self.assertEqual(self.parents(id),{5330})
        self.assertEqual(self.parents(5369),{5326})
        for id in (5370,13819,5340,5365,5380,12894,11596,13475):
            self.assertIsNone(self.after.execute('SELECT 1 FROM concept WHERE concept_id=?',(id,)).fetchone())
        self.assertEqual(self.after.execute('SELECT count(*) FROM concept WHERE concept_id IN(11757,11785)').fetchone()[0],2)

    def test_contained_ranges_do_not_bridge_gaps(self):
        self.assertTrue(self.manifest['removed_contained_ranges'])
        for id,lo,hi,outer_lo,outer_hi in self.manifest['removed_contained_ranges']:
            self.assertIsNone(self.after.execute('SELECT 1 FROM lcc_range WHERE start_code=? AND end_code=?',(lo,hi)).fetchone())
            self.assertEqual(self.after.execute('SELECT concept_id FROM lcc_range WHERE start_code=? AND end_code=?',(outer_lo,outer_hi)).fetchone(),(id,))

    def test_all_non_lcc_assignments_preserved_except_reviewed_moves(self):
        merges={int(k):v for k,v in self.manifest['merges'].items()}
        def destination(id):
            while id in merges:id=merges[id]
            return id
        for table in ('bisac_selector','ddc_selector'):
            old={(code,destination(id)) for code,id in self.before.execute(f'SELECT selector,concept_id FROM {table}')}
            if table=='ddc_selector':
                old.remove(('322',5696));old.add(('322',self.new['State & Civil Society']))
            self.assertEqual(old,set(self.after.execute(f'SELECT selector,concept_id FROM {table}')))

    def test_production_matcher_destinations(self):
        expected={
            'lcc:TS1065':self.new['Fur Manufacturing'],
            'lcc:JS7825':self.new['Atlantic & Indian Ocean Islands'],
            'lcc:JS8002':13380,'lcc:K3150':5548,'lcc:K3225':5330,
            'lcc:JZ6530':self.new['Humanitarian Aspects of War'],
            'lcc:JZ1256':5300,'lcc:JC321':5338,'lcc:JC574':11822,
            'lcc:JF2015':11785,'lcc:JK2300':11757,'lcc:JA50':52716,
            'bisac:POL053000':11884,'bisac:POL058000':5368,
            'bisac:POL040020':5326,'bisac:POL014000':5358,
            'ddc:322':self.new['State & Civil Society'],
        }
        result=subprocess.run([str(politics.ROOT/'target/release/taxonomy_probe'),str(FOLDER/'after.sqlite3'),'--code-only',*expected],check=True,text=True,capture_output=True)
        actual={r['system']+':'+r['code']:r['resolved'] for r in csv.DictReader(io.StringIO(result.stdout))}
        self.assertEqual(actual,{k:str(v) for k,v in expected.items()})

    def test_full_observed_audit_reviewed(self):
        # Normal merges are accounted for transitively by audit-politics.py.
        rows=json.loads((FOLDER/'full-nonmerge-changes.json').read_text())
        merges={int(k):v for k,v in self.manifest['merges'].items()}
        def canonical(id):
            while id in merges:id=merges[id]
            return id
        fallback_changes=[]
        for row in rows:
            old={canonical(i) for i in row['before']}
            new=set(row['after'])
            self.assertTrue(new,'No previously covered notation may become unmatched')
            if {5548 if id==5335 else id for id in old}==new:continue
            if (old,new) in [({13507},{self.new['Fur Manufacturing']}),
                            ({13380},{self.new['Atlantic & Indian Ocean Islands']}),
                            ({13233},{self.new['Humanitarian Aspects of War']})]:continue
            # Individually reviewed abbreviated/concatenated imports recover to
            # different shared parents after the geographical and Law moves.
            self.assertTrue(row['code'].endswith(('*','+','-')),row)
            self.assertLessEqual(old^new,{5690,12528,5691,54933,11917,11921,11923,5330,5693,5326,5055,5696,5374},row)
            fallback_changes.append(row)
        self.assertEqual(len(rows),2077)
        self.assertEqual(len(fallback_changes),52)

    def test_final_label_clarification_preserves_audited_matching_inputs(self):
        with sqlite3.connect(FOLDER/'matcher-audited.sqlite3') as audited:
            old,new=politics.snapshot(audited),politics.snapshot(self.after)
        for table in politics.TABLES:
            if table!='concept':self.assertEqual(old[table],new[table])
        self.assertLessEqual(set(old['concept'])-set(new['concept']),{(52866,'German Empire, 1867–1918')})
        self.assertLessEqual(set(new['concept'])-set(old['concept']),{(52866,'Confederation & Empire, 1867–1918')})
        self.assertEqual(self.label(52866),'Confederation & Empire, 1867–1918')

    @unittest.skipUnless(LIVE,'Use --live after applying the frozen plan')
    def test_live_database_matches_reviewed_snapshot(self):
        with sqlite3.connect(f'file:{politics.DATABASE}?mode=ro',uri=True) as live:
            self.assertEqual(politics.snapshot(live),politics.snapshot(self.after))


if __name__=='__main__':unittest.main()
