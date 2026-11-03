import unittest
from import_teams import pack

class ImportTests(unittest.TestCase):
    def paste(self, spread='EVs: 32 HP / 32 Atk / 2 SpD'):
        mon=f'Incineroar @ Sitrus Berry\nAbility: Intimidate\n{spread}\nAdamant Nature\n- Fake Out\n- Flare Blitz\n- Knock Off\n- Parting Shot'
        return '\n\n'.join([mon]*6)

    def test_champions_spread_preserved(self):
        packed,assumptions=pack(self.paste())
        self.assertEqual(len(packed.split(']')),6)
        self.assertEqual(packed.split(']')[0].split('|')[6],'32,32,0,0,2,0')
        self.assertFalse(assumptions)

    def test_conventional_evs_not_silently_reinterpreted(self):
        with self.assertRaisesRegex(ValueError,'non-Champions'):
            pack(self.paste('EVs: 252 HP / 252 Atk / 4 SpD'))

    def test_missing_spread_is_reported(self):
        _,assumptions=pack(self.paste().replace('EVs: 32 HP / 32 Atk / 2 SpD\n',''))
        self.assertEqual(len(assumptions),6)

    def test_nickname_and_gender(self):
        text=self.paste().replace('Incineroar @','Cat (Incineroar) (M) @')
        fields=pack(text)[0].split(']')[0].split('|')
        self.assertEqual(fields[0],'Incineroar')
        self.assertEqual(fields[7],'M')

if __name__=='__main__':unittest.main()
