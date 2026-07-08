import unittest
import pyqql

class TestPyQql(unittest.TestCase):
    def test_parse(self):
        query = "QUERY 'hello' FROM docs LIMIT 10"
        ast = pyqql.parse(query)
        self.assertIsInstance(ast, dict)
        self.assertIn("Query", ast)
        self.assertEqual(ast["Query"]["collection"], "docs")
        self.assertEqual(ast["Query"]["query_text"], "hello")

    def test_parse_batch(self):
        queries = ["QUERY 'test' FROM users LIMIT 5", "CREATE COLLECTION items"]
        results = pyqql.parse_batch(queries)
        self.assertEqual(len(results), 2)
        self.assertEqual(results[0]["Query"]["collection"], "users")
        self.assertIn("CreateCollection", results[1])
        
    def test_tokenize(self):
        tokens = pyqql.tokenize("QUERY 'test' FROM docs")
        self.assertTrue(len(tokens) > 0)
        self.assertEqual(tokens[0]["text"], "QUERY")
        
    def test_invalid(self):
        with self.assertRaises(SyntaxError):
            pyqql.parse("invalid syntax")
            
if __name__ == '__main__':
    unittest.main()
