import unittest
import refresh_catalog as catalog


class CatalogTests(unittest.TestCase):
    def test_new_upstream_titles_need_no_title_specific_mapping(self):
        rows = catalog.titles("| Future Game | CUSA99999 | x | Playable | New upstream route |\n"
                              "| Unsupported | CUSA99998 | x | Suspended | Not working |")
        self.assertEqual([row["title_id"] for row in rows], ["CUSA99999"])
        self.assertEqual(rows[0]["notes"], "New upstream route")

    def test_artwork_uses_official_metadata_and_falls_back_to_cover(self):
        cover = "https://image.api.playstation.com/fixture.png"
        page = '<script type="application/ld+json">{"image":"' + cover + '"}</script>'
        self.assertEqual(catalog.artwork(page), {"cover": cover, "hero": cover})

    def test_artwork_rejects_an_unexpected_publisher(self):
        page = '<script type="application/ld+json">{"image":"https://example.com/image"}</script>'
        with self.assertRaises(ValueError):
            catalog.artwork(page)


if __name__ == "__main__":
    unittest.main()
