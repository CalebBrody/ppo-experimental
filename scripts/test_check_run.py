"""Regression cases for the issue checker, including event attribution."""
import unittest
import json
from pathlib import Path
import tempfile
from check_run import analyze_game, inspect_evaluation, online_summary


class CheckerTests(unittest.TestCase):
    def test_online_only_audit_uses_training_counters_without_evaluations(self):
        r=online_summary([{'online_audit':{'window_samples':128,'counts':{'observed_immunity_events':3,'dual_target_1':60,'dual_target_2':70}}}])
        self.assertEqual(r['issues']['observed_immunity_events'],'observed')
        self.assertEqual(r['issues']['target_collapse'],'not_observed_in_sample')
        self.assertEqual(r['issues']['preview_index_collapse'],'insufficient_data')
        self.assertIsNone(online_summary([{'online_audit':None}]))
    def game(self, action, own=None, foes=None, terrain=None, events=None):
        return dict(game=3,learner_side=0,decisions=[dict(turn=2,request='Move',
            selected=[action],state={'sides':[own or [],foes or []],'terrain':terrain},events=events or [])])

    def test_progress_failure_is_known_before_selection(self):
        action={'actor':'Pawmot','actor_slot':0,'move':'Double Shock','target':2}
        own=[{'slot':0,'types':['Fighting']}]
        counts,_,_=analyze_game(self.game(action,own),{})
        self.assertEqual(counts['double_shock_without_electric'],1)
        own[0]['types'].append('Electric')
        self.assertEqual(analyze_game(self.game(action,own),{})[0]['double_shock_without_electric'],0)

    def test_terrain_and_first_action_rules(self):
        action={'move':'Steel Roller','target':2,'actor_slot':0}
        self.assertEqual(analyze_game(self.game(action),{})[0]['steel_roller_without_terrain'],1)
        self.assertEqual(analyze_game(self.game(action,terrain='grassyterrain'),{})[0]['steel_roller_without_terrain'],0)
        action={'move':'Fake Out','target':2,'active_move_actions':1}
        self.assertEqual(analyze_game(self.game(action),{})[0]['first_turn_move_after_first_action'],1)

    def test_target_opportunity_requires_two_live_foes(self):
        action={'move':'Psychic','target':2,'both_foe_targets_legal':True}
        foes=[{'slot':0,'hp':100},{'slot':1,'hp':100}]
        self.assertEqual(analyze_game(self.game(action,foes=foes),{})[0]['dual_target_opportunities'],1)
        foes[0]['fainted']=True
        self.assertEqual(analyze_game(self.game(action,foes=foes),{})[0]['dual_target_opportunities'],0)

    def test_immunity_is_attributed_to_attacker_not_defender(self):
        events=['|move|p1a: Sneasler|Close Combat|p2b: Gholdengo','|-immune|p2b: Gholdengo',
                '|move|p2a: Gengar|Shadow Ball|p1b: Indeedee','|-immune|p1b: Indeedee',
                '|-fail|p1b: Indeedee|unboost|atk|[from] ability: Inner Focus']
        counts,_,_=analyze_game(self.game({'pass':True},events=events),{})
        self.assertEqual(counts['observed_immunity_events'],1)
        self.assertEqual(counts['executed_move_attempts'],1)
        self.assertEqual(counts['failed_move_events'],0)

    def test_scrappy_and_normal_type_conversion(self):
        action={'move':'Double-Edge','type':'Normal','category':'Physical','target':2,'actor_slot':0}
        own=[{'slot':0,'ability':'Pixilate'}]
        foes=[{'slot':1,'types':['Ghost'],'species':'Gengar'}]
        dex={'typechart':{'Ghost':{'Normal':3,'Fairy':0}},'moves':{}}
        self.assertEqual(analyze_game(self.game(action,own,foes),dex)[0]['visible_type_immunity_candidate'],0)
        own[0]['ability']='Pressure'
        self.assertEqual(analyze_game(self.game(action,own,foes),dex)[0]['visible_type_immunity_candidate'],1)

    def test_struggle_recharge_has_nullable_move_metadata(self):
        action={'move':'Struggle/recharge','target':0,'accuracy':None,'power':None,'category':None,'type':None}
        counts,_,_=analyze_game(self.game(action),{})
        self.assertEqual(counts['selected_moves'],1)
        self.assertEqual(counts['low_accuracy_selections'],0)

    def test_known_immunity_requires_same_visible_target_not_a_switch_in(self):
        action={'move':'Close Combat','type':'Fighting','category':'Physical','target':2,'actor_slot':0}
        own=[{'slot':0,'ability':'Poison Touch'}]
        foes=[{'slot':1,'types':['Ghost','Steel'],'species':'Gholdengo','name':'Gholdengo'}]
        dex={'typechart':{'Ghost':{'Fighting':3}},'moves':{}}
        events=['|move|p1a: Sneasler|Close Combat|p2b: Gholdengo','|-immune|p2b: Gholdengo']
        counts,_,_=analyze_game(self.game(action,own,foes,events=events),dex)
        self.assertEqual(counts['known_type_immunity_events'],1)
        events[-1]='|-immune|p2b: Gengar'
        counts,_,_=analyze_game(self.game(action,own,foes,events=events),dex)
        self.assertEqual(counts['observed_immunity_events'],1)
        self.assertEqual(counts['known_type_immunity_events'],0)

    def test_risk_states_count_exposure_even_when_policy_chooses_another_move(self):
        own=[{'slot':0,'types':['Fighting'],'hp':80,'moves':[{'move':'Double Shock','pp':5}]}]
        counts,_,_=analyze_game(self.game({'move':'Mach Punch','actor_slot':0,'target':2},own),{})
        self.assertEqual(counts['double_shock_risk_states'],1)
        self.assertEqual(counts['double_shock_without_electric'],0)
        own[0]['moves'][0]['pp']=0
        self.assertEqual(analyze_game(self.game({'pass':True},own),{})[0]['double_shock_risk_states'],0)

    def test_missing_traces_are_untested_and_invalid_indices_fail_coverage(self):
        workspace = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(dir=workspace/'runs',prefix='checker-test-') as folder:
            path=Path(folder).resolve()
            self.assertTrue(path.is_relative_to(workspace))
            (path/'evaluation.json').write_text(json.dumps(dict(games=1,wins=1,losses=0,draws=0,results=[dict(game=3,score=1.)])))
            report=inspect_evaluation(path,{})
            self.assertFalse(report['trace_complete'])
            self.assertEqual(report['issues']['immunity_attacks'],'untested')
            (path/'traces').mkdir()
            game=self.game({'pass':True});game.update(outcome=0,team_a=0,team_b=1)
            game['decisions'][0].update(selected_index=9,legal_actions=3)
            (path/'traces/game-0003.json').write_text(json.dumps(game))
            report=inspect_evaluation(path,{})
            self.assertFalse(report['trace_complete'])
            self.assertTrue(any('Illegal selected action' in error for error in report['errors']))


if __name__=='__main__':
    unittest.main()
